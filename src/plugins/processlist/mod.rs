//! Process list — per-process details for every visible PID.
//!
//! Mirrors `glances/plugins/processlist/__init__.py`. Linux-only.
//! Each refresh walks numeric `/proc` entries and publishes one object
//! per process: pid, name, cmdline, username, threads, cpu/memory
//! percentages, `memory_info`, `cpu_times`, `io_counters`, and cpu_num.
//!
//! Notes on std-only approximations (no libc NSS, no psutil):
//! * `cpu_percent` is the utime+stime delta over the `/proc/stat` total
//!   delta between ticks (first tick reports 0.0) — no HZ constant needed.
//! * `username` comes from parsing `/etc/passwd`; unknown uids fall back
//!   to the numeric id (LDAP/NSS users won't resolve).
//! * `rss`/`vms` bytes use the kernel page size via `sysconf(3)` with a
//!   4096 fallback, matching the direct-libc precedent in `platform/`.
//! * Unreadable processes (other users, no permission) are skipped.

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
pub use sample::{sample_to_value, ProcSample};


/// Kernel page size via `platform::linux::sysconf` (raw `unsafe` lives
/// there per AC-11; 4096 fallback on failure).
pub fn page_size() -> u64 {
    plat::linux::sysconf::page_size()
}


pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(ProcessListPlugin::new()));
}

pub struct ProcessListPlugin {
    base: GlancesPluginModel,
    prev: HashMap<u32, (u64, u64)>,
}

impl ProcessListPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            prev: HashMap::new(),
        }
    }
}

/// Parse `/proc/<pid>/stat` tail (after the `(comm)` field).
/// Returns (comm, state, utime, stime, nice, num_threads, cpu_num).
/// Field numbers per proc(5): state=3, utime=14, stime=15, nice=19,
/// num_threads=20, processor=39.
/// Map a state char to a psutil-style status name.
pub fn status_name(state: char) -> &'static str {
    match state {
        'R' => "running",
        'S' => "sleeping",
        'D' => "disk-sleep",
        'T' | 't' => "stopped",
        'Z' | 'X' | 'x' => "zombie",
        'I' => "idle",
        _ => "unknown",
    }
}

/// Sample every visible process. `prev` maps pid → (proc_ticks, total_ticks)
/// from the last call and is updated in place; entries for exited pids are
/// pruned. First sight of a pid reports `cpu_percent` 0.0.
pub fn sample_all(prev: &mut HashMap<u32, (u64, u64)>) -> Vec<ProcSample> {
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
        let proc_ticks = utime.saturating_add(stime);
        let cpu_percent = match prev.get(&pid) {
            Some((pt, tt)) if total > *tt => {
                let dp = proc_ticks.saturating_sub(*pt) as f64;
                let dt = total.saturating_sub(*tt) as f64;
                if dt > 0.0 {
                    (dp / dt * 100.0).max(0.0)
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        prev.insert(pid, (proc_ticks, total));
        let (state_c, uid, gids) = parse_status_file(pid).unwrap_or((state, 0, (0, 0, 0)));
        let (vms, rss, mem_shared, mem_text, mem_lib, mem_data, mem_dirty) =
            parse_statm(pid, page).unwrap_or((0, 0, 0, 0, 0, 0, 0));
        let (read_bytes, write_bytes, read_count, write_count) = parse_io(pid);
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
            cpu_num,
        });
    }
    prev.retain(|pid, _| out.iter().any(|p| p.pid == *pid));
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
        let samples = sample_all(&mut self.prev);
        self.base.stats = Value::Array(samples.iter().map(sample_to_value).collect());
        Ok(())
    }
    fn update_views(&mut self, _events: &mut EventLog) {
        // Upstream processlist update_views: views stay empty (per-
        // process decorations are built lazily by API consumers).
        if let Some(m) = self.model_mut() {
            m.views = std::collections::HashMap::new();
        }
    }
}
