//! Program list — processes grouped by program name.
//!
//! Mirrors `glances/plugins/programlist/__init__.py` (which derives from
//! the process list via `as_programs=True`). Linux-only. Each refresh
//! reuses the `processlist` sampler and collapses rows sharing a process
//! name: threads/cpu/memory/times/io are summed, `nprocs` counts members,
//! `childrens` holds member pids, and username/nice/status collapse to
//! `"_"` when members disagree.

use std::collections::{BTreeMap, HashMap};

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::plugins::processlist::{self, ProcSample};

pub const NAME: &str = "programlist";

/// Collapse marker used when grouped processes disagree on a field.
pub const DISAGREE: &str = "_";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(ProgramListPlugin::new()));
}

pub struct ProgramListPlugin {
    base: GlancesPluginModel,
    prev: HashMap<u32, (u64, u64, u64, u64)>,
    seen: HashMap<u32, std::time::Instant>,
}

impl Default for ProgramListPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgramListPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            prev: HashMap::new(),
            seen: HashMap::new(),
        }
    }
}

/// One aggregated program row.
#[derive(Debug, Clone, Default)]
pub struct ProgramRow {
    pub name: String,
    pub cmdline: String,
    pub username: String,
    pub nprocs: u64,
    pub num_threads: u64,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub rss: u64,
    pub vms: u64,
    pub utime: u64,
    pub stime: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub status: String,
    pub nice: String,
    pub childrens: Vec<u32>,
}

/// Group samples by process name, summing counters. Fields that differ
/// across members collapse to `"_"`.
pub fn aggregate(samples: &[ProcSample]) -> Vec<ProgramRow> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&ProcSample>> = HashMap::new();
    for s in samples {
        groups
            .entry(s.name.clone())
            .or_insert_with(|| {
                order.push(s.name.clone());
                Vec::new()
            })
            .push(s);
    }
    let mut rows = Vec::new();
    for name in order {
        let members = &groups[&name];
        let first = members[0];
        let agree_username = members.iter().all(|m| m.username == first.username);
        let agree_status = members.iter().all(|m| m.state == first.state);
        let agree_nice = members.iter().all(|m| m.nice == first.nice);
        rows.push(ProgramRow {
            name: name.clone(),
            cmdline: first.cmdline.join(" "),
            username: if agree_username {
                first.username.clone()
            } else {
                DISAGREE.to_string()
            },
            nprocs: members.len() as u64,
            num_threads: members.iter().map(|m| m.num_threads).sum(),
            cpu_percent: members.iter().map(|m| m.cpu_percent).sum(),
            memory_percent: members.iter().map(|m| m.memory_percent).sum(),
            rss: members.iter().map(|m| m.rss).sum(),
            vms: members.iter().map(|m| m.vms).sum(),
            utime: members.iter().map(|m| m.utime).sum(),
            stime: members.iter().map(|m| m.stime).sum(),
            read_bytes: members.iter().map(|m| m.read_bytes).sum(),
            write_bytes: members.iter().map(|m| m.write_bytes).sum(),
            status: if agree_status {
                processlist::status_name(first.state).to_string()
            } else {
                DISAGREE.to_string()
            },
            nice: if agree_nice {
                first.nice.to_string()
            } else {
                DISAGREE.to_string()
            },
            childrens: members.iter().map(|m| m.pid).collect(),
        });
    }
    rows.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

/// Render one aggregated row as a stats object.
pub fn row_to_value(r: &ProgramRow) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("name".into(), Value::String(r.name.clone()));
    obj.insert("cmdline".into(), Value::String(r.cmdline.clone()));
    obj.insert("username".into(), Value::String(r.username.clone()));
    obj.insert("nprocs".into(), Value::Uint(r.nprocs));
    obj.insert("num_threads".into(), Value::Uint(r.num_threads));
    obj.insert(
        "cpu_percent".into(),
        Value::Float((r.cpu_percent * 100.0).round() / 100.0),
    );
    obj.insert(
        "memory_percent".into(),
        Value::Float((r.memory_percent * 100.0).round() / 100.0),
    );
    let mut mem = BTreeMap::new();
    mem.insert("rss".into(), Value::Uint(r.rss));
    mem.insert("vms".into(), Value::Uint(r.vms));
    obj.insert("memory_info".into(), Value::Object(mem));
    obj.insert("status".into(), Value::String(r.status.clone()));
    obj.insert("nice".into(), Value::String(r.nice.clone()));
    let mut times = BTreeMap::new();
    times.insert("user".into(), Value::Uint(r.utime));
    times.insert("system".into(), Value::Uint(r.stime));
    obj.insert("cpu_times".into(), Value::Object(times));
    let mut io = BTreeMap::new();
    io.insert("read_bytes".into(), Value::Uint(r.read_bytes));
    io.insert("write_bytes".into(), Value::Uint(r.write_bytes));
    obj.insert("io_counters".into(), Value::Object(io));
    obj.insert(
        "childrens".into(),
        Value::Array(
            r.childrens
                .iter()
                .map(|p| Value::Uint(*p as u64))
                .collect(),
        ),
    );
    Value::Object(obj)
}

impl Plugin for ProgramListPlugin {
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
        Some("name")
    }

    fn update(&mut self) -> Result<()> {
        if !cfg!(target_os = "linux") {
            self.base.stats = Value::Array(Vec::new());
            return Ok(());
        }
        let now = std::time::Instant::now();
        let samples = processlist::sample_all(&mut self.prev, &mut self.seen, now);
        let rows = aggregate(&samples);
        self.base.stats = Value::Array(rows.iter().map(row_to_value).collect());
        Ok(())
    }
}
