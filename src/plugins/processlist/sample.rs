//! Process samples: the `ProcSample` row type plus stats rendering.
//!
//! Raw tick counters live on the sample; percentages are derived
//! against the caller's previous snapshot in `sample_all`.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::platform as plat;

#[derive(Debug, Clone, Default)]
pub struct ProcSample {
    pub pid: u32,
    pub name: String,
    /// argv list (`cmdline` is an array on the wire, not a string).
    pub cmdline: Vec<String>,
    pub username: String,
    pub num_threads: u64,
    pub state: char,
    pub nice: i64,
    /// Gids (real, effective, saved) from `/proc/<pid>/status`.
    pub gids: (u32, u32, u32),
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub rss: u64,
    pub vms: u64,
    pub mem_shared: u64,
    pub mem_text: u64,
    pub mem_lib: u64,
    pub mem_data: u64,
    pub mem_dirty: u64,
    pub utime: u64,
    pub stime: u64,
    /// Block-I/O delay ticks (the `cpu_times.iowait` source).
    pub iowait_ticks: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_count: u64,
    pub write_count: u64,
    /// Per-second I/O rates, derived against the previous sample.
    pub read_rate: f64,
    pub write_rate: f64,
    pub cpu_num: u64,
    /// Seconds since this pid's previous sighting (0.0 on first sight).
    pub time_since_update: f64,
}

/// Render one sample as a stats object. Percentages round to 2dp.
pub fn sample_to_value(p: &ProcSample) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("pid".into(), Value::Uint(p.pid as u64));
    obj.insert("key".into(), Value::String("pid".into()));
    obj.insert(
        "time_since_update".into(),
        Value::Float(p.time_since_update.max(0.0)),
    );
    obj.insert("name".into(), Value::String(p.name.clone()));
    obj.insert(
        "cmdline".into(),
        Value::Array(p.cmdline.iter().cloned().map(Value::String).collect()),
    );
    obj.insert("username".into(), Value::String(p.username.clone()));
    let mut gids = BTreeMap::new();
    gids.insert("real".into(), Value::Uint(p.gids.0 as u64));
    gids.insert("effective".into(), Value::Uint(p.gids.1 as u64));
    gids.insert("saved".into(), Value::Uint(p.gids.2 as u64));
    obj.insert("gids".into(), Value::Object(gids));
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
    mem.insert("shared".into(), Value::Uint(p.mem_shared));
    mem.insert("text".into(), Value::Uint(p.mem_text));
    mem.insert("lib".into(), Value::Uint(p.mem_lib));
    mem.insert("data".into(), Value::Uint(p.mem_data));
    mem.insert("dirty".into(), Value::Uint(p.mem_dirty));
    obj.insert("memory_info".into(), Value::Object(mem));
    obj.insert("status".into(), Value::String(status_name(p.state).to_string()));
    obj.insert("nice".into(), Value::Int(p.nice));
    let mut times = BTreeMap::new();
    times.insert("user".into(), Value::Uint(p.utime));
    times.insert("system".into(), Value::Uint(p.stime));
    times.insert("iowait".into(), Value::Uint(p.iowait_ticks));
    obj.insert("cpu_times".into(), Value::Object(times));
    let mut io = BTreeMap::new();
    io.insert("read_count".into(), Value::Uint(p.read_count));
    io.insert("write_count".into(), Value::Uint(p.write_count));
    io.insert("read_bytes".into(), Value::Uint(p.read_bytes));
    io.insert("write_bytes".into(), Value::Uint(p.write_bytes));
    obj.insert("io_counters".into(), Value::Object(io));
    obj.insert("disk_read_rate_per_sec".into(), Value::Float(p.read_rate));
    obj.insert("disk_write_rate_per_sec".into(), Value::Float(p.write_rate));
    obj.insert("cpu_num".into(), Value::Uint(p.cpu_num));
    Value::Object(obj)
}

/// Irix mode (`-0`): per-process CPU% divided by core count.
pub fn divide_cpu_percent(v: &mut Value) {
    let n = plat::linux::proc_cpuinfo::cpu_count().max(1) as f64;
    if let Some(o) = v.as_object_mut()
        && let Some(p) = o.get("cpu_percent").and_then(|x| x.as_f64()) {
            o.insert("cpu_percent".into(), Value::Float(p / n));
        }
}

/// Single-letter state → dashboard word.
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
