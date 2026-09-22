//! Process list — per-process details for every visible PID.
//!
//! Mirrors `glances/plugins/processlist/__init__.py`. Linux-only.
//! Each refresh walks numeric `/proc` entries and publishes one object
//! per process: pid, name, cmdline, username, threads, cpu/memory
//! percentages, `memory_info`, `cpu_times`, `io_counters`, per-second
//! disk rates, and cpu_num.
//!
//! std-only approximations: cpu% from /proc deltas (first tick 0.0),
//! usernames from /etc/passwd (numeric fallback), rss via the sysconf
//! page size (4096 fallback); unreadable processes are skipped.

use std::collections::HashMap;
use std::fs;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

mod read;
mod sample;

pub const NAME: &str = "processlist";

pub use read::{build_user_map, parse_io, parse_io_text, parse_stat_fields, parse_statm, parse_statm_text, parse_status_file, parse_status_text, read_cmdline, read_total_cpu};
pub use sample::{divide_cpu_percent, sample_to_value, status_name, ProcSample};


/// Kernel page size via sysconf (platform FFI; 4096 fallback).
pub fn page_size() -> u64 {
    plat::linux::sysconf::page_size()
}


pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(ProcessListPlugin::new()));
}

pub struct ProcessListPlugin {
    base: GlancesPluginModel,
    prev: HashMap<u32, (u64, u64, u64, u64)>,
    /// Last-seen instant per pid (per-process `time_since_update`).
    prev_seen: HashMap<u32, std::time::Instant>,
    /// Display filter (`-f/--process-filter` parity). Empty = show all.
    filter: crate::core::filter::GlancesFilterList,
    irix_divide: bool,
}

impl ProcessListPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            prev: HashMap::new(),
            prev_seen: HashMap::new(),
            filter: crate::core::filter::GlancesFilterList::new(),
            irix_divide: false,
        }
    }

    /// Replace the display filter (upstream `process_filter` setter).
    pub fn apply_process_filter(&mut self, raw: Option<&str>) {
        match raw {
            None => self.filter.clear(),
            Some(s) => self.filter.set_filter(s),
        }
    }
}

/// Sample every visible process; `prev` maps pid → (proc ticks,
/// total ticks, read bytes, write bytes), `seen` tracks last sight.
/// First sight of a pid reports zero cpu% and I/O rates.
pub fn sample_all(
    prev: &mut HashMap<u32, (u64, u64, u64, u64)>,
    seen: &mut HashMap<u32, std::time::Instant>,
    now: std::time::Instant,
) -> Vec<ProcSample> {
    let total = read_total_cpu();
    let mem_total = plat::linux::proc_meminfo::read().map(|m| m.total).unwrap_or(0);
    let page = page_size();
    let users = build_user_map();
    let mut out = Vec::new();
    let entries = match fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return out,
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let pid: u32 = match name.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let stat = match fs::read_to_string(format!("/proc/{}/stat", pid)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let (comm, state, utime, stime, nice, threads, cpu_num, blkio_ticks) =
            match parse_stat_fields(&stat) {
                Some(v) => v,
                None => continue,
            };
        let (read_bytes, write_bytes, read_count, write_count) = parse_io(pid);
        let proc_ticks = utime.saturating_add(stime);
        let elapsed = seen
            .get(&pid)
            .map(|t| now.duration_since(*t).as_secs_f64().max(0.0))
            .unwrap_or(0.0);
        let (cpu_percent, read_rate, write_rate) = match prev.get(&pid) {
            Some((pt, tt, pr, pw)) => {
                let cpu = if total > *tt {
                    let dt = total.saturating_sub(*tt) as f64;
                    if dt > 0.0 {
                        (proc_ticks.saturating_sub(*pt) as f64 / dt * 100.0).max(0.0)
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                let (r, w) = if elapsed > 0.0 {
                    (
                        read_bytes.saturating_sub(*pr) as f64 / elapsed,
                        write_bytes.saturating_sub(*pw) as f64 / elapsed,
                    )
                } else {
                    (0.0, 0.0)
                };
                (cpu, r, w)
            }
            _ => (0.0, 0.0, 0.0),
        };
        prev.insert(pid, (proc_ticks, total, read_bytes, write_bytes));
        seen.insert(pid, now);
        let (state_c, uid, gids) = parse_status_file(pid).unwrap_or((state, 0, (0, 0, 0)));
        let (vms, rss, mem_shared, mem_text, mem_lib, mem_data, mem_dirty) =
            parse_statm(pid, page).unwrap_or((0, 0, 0, 0, 0, 0, 0));
        let cmdline = read_cmdline(pid);
        let username = users
            .get(&uid)
            .cloned()
            .unwrap_or_else(|| uid.to_string());
        let memory_percent = if mem_total > 0 {
            rss as f64 / mem_total as f64 * 100.0
        } else {
            0.0
        };
        out.push(ProcSample {
            pid,
            name: comm,
            cmdline,
            username,
            num_threads: threads,
            state: state_c,
            nice,
            gids,
            cpu_percent,
            memory_percent,
            rss,
            vms,
            mem_shared,
            mem_text,
            mem_lib,
            mem_data,
            mem_dirty,
            utime,
            stime,
            iowait_ticks: blkio_ticks,
            read_bytes,
            write_bytes,
            read_count,
            write_count,
            read_rate,
            write_rate,
            cpu_num,
            time_since_update: elapsed,
        });
    }
    prev.retain(|pid, _| out.iter().any(|p| p.pid == *pid));
    seen.retain(|pid, _| out.iter().any(|p| p.pid == *pid));
    out.sort_by_key(|p| p.pid);
    out
}

/// Render one sample as a stats object.
impl Plugin for ProcessListPlugin {
    fn name(&self) -> &'static str {
        NAME
    }
    fn reset(&mut self) {
        self.base.reset();
    }
    fn stats(&self) -> &Value {
        &self.base.stats
    }
    fn model(&self) -> Option<&GlancesPluginModel> {
        Some(&self.base)
    }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> {
        Some(&mut self.base)
    }
    fn stats_mut(&mut self) -> &mut Value {
        &mut self.base.stats
    }
    fn get_key(&self) -> Option<&'static str> {
        Some("pid")
    }

    fn update(&mut self) -> Result<()> {
        if !cfg!(target_os = "linux") {
            self.base.stats = Value::Array(Vec::new());
            return Ok(());
        }
        let now = std::time::Instant::now();
        let samples = sample_all(&mut self.prev, &mut self.prev_seen, now);
        let mut out = Vec::new();
        for s in samples {
            let mut v = sample_to_value(&s);
            if self.irix_divide {
                divide_cpu_percent(&mut v);
            }
            // Display filter (upstream `get_list` `_filter` parity).
            if !self.filter.is_empty() {
                let show = match &v {
                    Value::Object(o) => self.filter.is_filtered(o),
                    _ => true,
                };
                if !show {
                    continue;
                }
            }
            out.push(v);
        }
        self.base.stats = Value::Array(out);
        Ok(())
    }
    fn set_process_filter(&mut self, raw: Option<&str>) {
        self.apply_process_filter(raw);
    }
    fn set_irix_divide(&mut self, divide: bool) {
        self.irix_divide = divide;
    }
    fn update_views(&mut self, _events: &mut EventLog) {
        // Upstream processlist update_views: views stay empty (per-
        // process decorations are built lazily by API consumers).
        if let Some(m) = self.model_mut() {
            m.views = std::collections::HashMap::new();
        }
    }
}
