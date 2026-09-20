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

use std::collections::{BTreeMap, HashMap};
use std::fs;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "processlist";

extern "C" {
    fn sysconf(name: i32) -> i64;
}
const SC_PAGESIZE: i32 = 30;

/// Kernel page size in bytes (4096 fallback if `sysconf` fails).
pub fn page_size() -> u64 {
    let v = unsafe { sysconf(SC_PAGESIZE) };
    if v > 0 {
        v as u64
    } else {
        4096
    }
}

/// One sampled process. Raw tick counters stay here; percentages are
/// derived against the caller's previous snapshot.
#[derive(Debug, Clone, Default)]
pub struct ProcSample {
    pub pid: u32,
    pub name: String,
    pub cmdline: String,
    pub username: String,
    pub num_threads: u64,
    pub state: char,
    pub nice: i64,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub rss: u64,
    pub vms: u64,
    pub utime: u64,
    pub stime: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub cpu_num: u64,
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
pub fn parse_stat_fields(line: &str) -> Option<(String, char, u64, u64, i64, u64, u64)> {
    let sp = line.find(' ')?;
    let rest = &line[sp + 1..];
    let open = rest.find('(')?;
    let close = rest.rfind(')')?;
    if close <= open {
        return None;
    }
    let comm = rest[open + 1..close].to_string();
    let tail: Vec<&str> = rest[close + 1..].split_whitespace().collect();
    // Need indices through nice/threads (17); processor (36) is optional.
    if tail.len() < 18 {
        return None;
    }
    let state = tail[0].chars().next()?;
    let utime = tail[11].parse::<u64>().ok()?;
    let stime = tail[12].parse::<u64>().ok()?;
    let nice = tail[16].parse::<i64>().ok()?;
    let threads = tail[17].parse::<u64>().ok()?;
    let cpu_num = tail.get(36).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    Some((comm, state, utime, stime, nice, threads, cpu_num))
}

/// Parse `/proc/<pid>/status`: (state_char, uid). Missing file → None.
pub fn parse_status_file(pid: u32) -> Option<(char, u32)> {
    let text = fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    let mut state = None;
    let mut uid = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("State:") {
            state = rest.trim().chars().next();
        } else if let Some(rest) = line.strip_prefix("Uid:") {
            uid = rest.split_whitespace().next()?.parse::<u32>().ok();
        }
        if state.is_some() && uid.is_some() {
            break;
        }
    }
    Some((state?, uid?))
}

/// Parse `/proc/<pid>/statm`: (vms_bytes, rss_bytes).
pub fn parse_statm(pid: u32, page: u64) -> Option<(u64, u64)> {
    let text = fs::read_to_string(format!("/proc/{}/statm", pid)).ok()?;
    let mut it = text.split_whitespace();
    let size = it.next()?.parse::<u64>().ok()?;
    let resident = it.next()?.parse::<u64>().ok()?;
    Some((size.saturating_mul(page), resident.saturating_mul(page)))
}

/// Parse `/proc/<pid>/io`: (read_bytes, write_bytes). Missing → (0, 0).
pub fn parse_io(pid: u32) -> (u64, u64) {
    let text = match fs::read_to_string(format!("/proc/{}/io", pid)) {
        Ok(t) => t,
        Err(_) => return (0, 0),
    };
    let mut read = 0;
    let mut write = 0;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("read_bytes:") {
            read = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("write_bytes:") {
            write = rest.trim().parse().unwrap_or(0);
        }
    }
    (read, write)
}

/// Read `/proc/<pid>/cmdline` (NUL-separated) joined with spaces.
/// Empty (kernel threads) → empty string.
pub fn read_cmdline(pid: u32) -> String {
    match fs::read(format!("/proc/{}/cmdline", pid)) {
        Ok(bytes) => bytes
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect::<Vec<_>>()
            .join(" "),
        Err(_) => String::new(),
    }
}

/// Map uid → username via `/etc/passwd`. Unknown → numeric id string.
pub fn build_user_map() -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let text = match fs::read_to_string("/etc/passwd") {
        Ok(t) => t,
        Err(_) => return map,
    };
    for line in text.lines() {
        let mut parts = line.split(':');
        let name = match parts.next() {
            Some(n) => n,
            None => continue,
        };
        let _ = parts.next();
        if let Some(uid) = parts.next().and_then(|s| s.parse::<u32>().ok()) {
            map.entry(uid).or_insert_with(|| name.to_string());
        }
    }
    map
}

/// Sum the aggregate `cpu ` line in `/proc/stat` (all counters).
pub fn read_total_cpu() -> u64 {
    let text = match fs::read_to_string("/proc/stat") {
        Ok(t) => t,
        Err(_) => return 0,
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("cpu ") {
            return rest
                .split_whitespace()
                .filter_map(|f| f.parse::<u64>().ok())
                .fold(0u64, |a, b| a.saturating_add(b));
        }
    }
    0
}

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
        let (comm, state, utime, stime, nice, threads, cpu_num) =
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
        let (state_c, uid) = parse_status_file(pid).unwrap_or((state, 0));
        let (vms, rss) = parse_statm(pid, page).unwrap_or((0, 0));
        let (read_bytes, write_bytes) = parse_io(pid);
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
            cpu_percent,
            memory_percent,
            rss,
            vms,
            utime,
            stime,
            read_bytes,
            write_bytes,
            cpu_num,
        });
    }
    prev.retain(|pid, _| out.iter().any(|p| p.pid == *pid));
    out.sort_by_key(|p| p.pid);
    out
}

/// Render one sample as a stats object.
pub fn sample_to_value(p: &ProcSample) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("pid".into(), Value::Uint(p.pid as u64));
    obj.insert("name".into(), Value::String(p.name.clone()));
    obj.insert("cmdline".into(), Value::String(p.cmdline.clone()));
    obj.insert("username".into(), Value::String(p.username.clone()));
    obj.insert("num_threads".into(), Value::Uint(p.num_threads));
    obj.insert(
        "cpu_percent".into(),
        Value::Float((p.cpu_percent * 100.0).round() / 100.0),
    );
    obj.insert(
        "memory_percent".into(),
        Value::Float((p.memory_percent * 100.0).round() / 100.0),
    );
    let mut mem = BTreeMap::new();
    mem.insert("rss".into(), Value::Uint(p.rss));
    mem.insert("vms".into(), Value::Uint(p.vms));
    obj.insert("memory_info".into(), Value::Object(mem));
    obj.insert(
        "status".into(),
        Value::String(status_name(p.state).to_string()),
    );
    obj.insert("nice".into(), Value::Int(p.nice));
    let mut times = BTreeMap::new();
    times.insert("user".into(), Value::Uint(p.utime));
    times.insert("system".into(), Value::Uint(p.stime));
    obj.insert("cpu_times".into(), Value::Object(times));
    let mut io = BTreeMap::new();
    io.insert("read_bytes".into(), Value::Uint(p.read_bytes));
    io.insert("write_bytes".into(), Value::Uint(p.write_bytes));
    obj.insert("io_counters".into(), Value::Object(io));
    obj.insert("cpu_num".into(), Value::Uint(p.cpu_num));
    Value::Object(obj)
}

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
}
