//! Virtual machines — libvirt domains and Multipass instances.
//!
//! Mirrors `glances/plugins/vms/__init__.py`. Linux-only. Two engines,
//! both probed argv-only with no shell (same pattern as
//! `core/actions.rs`):
//! * `virsh`: `list --all` for names/states plus `domstats` for
//!   cpu.time (ns, rate-converted on the second tick), vCPU count, and
//!   balloon memory (KiB → bytes).
//! * `multipass`: `list --format csv` for name/state/ipv4/release
//!   (CPU time is not exposed by Multipass, so no cpu fields).
//!
//! Missing binaries or permissions yield an empty list — never an error.

use std::collections::{BTreeMap, HashMap};

use std::sync::OnceLock;
use std::time::Instant;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

mod parse;

pub const NAME: &str = "vms";

pub use parse::{parse_domstats, parse_multipass_csv, parse_virsh_list, VmRow};
use parse::{find_bin, run};


pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(VmsPlugin::new()));
}

pub struct VmsPlugin {
    base: GlancesPluginModel,
    prev_cpu: HashMap<String, (u128, Instant)>,
    virsh_version: OnceLock<String>,
    multipass_version: OnceLock<String>,
    /// Last sweep, re-rendered inside the freshness window (same
    /// 60s cache as SMART — VM lists move slowly; per-VM cpu% is a
    /// rate over real elapsed time, so it stays correct, just
    /// coarser).
    cached: Vec<VmRow>,
    collected_at: Option<Instant>,
}

impl Default for VmsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl VmsPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            prev_cpu: HashMap::new(),
            virsh_version: OnceLock::new(),
            multipass_version: OnceLock::new(),
            cached: Vec::new(),
            collected_at: None,
        }
    }
}

/// cpu.time rate; it is pruned to live domains.
pub fn collect_virsh(
    prev: &mut HashMap<String, (u128, Instant)>,
    version: &OnceLock<String>,
) -> Vec<VmRow> {
    let bin = match find_bin(&["/usr/bin", "/bin", "/usr/sbin", "/sbin"], "virsh") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let list = match run(&bin, &["list", "--all"]) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let stats = run(&bin, &["domstats", "--nowait"]).unwrap_or_default();
    let domstats = parse_domstats(&stats);
    let ver = version
        .get_or_init(|| {
            run(&bin, &["--version"])
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        })
        .clone();
    let now = Instant::now();
    let mut out = Vec::new();
    for (name, state) in parse_virsh_list(&list) {
        let d = domstats.get(&name);
        let cpu_percent = d
            .and_then(|m| m.get("cpu.time"))
            .and_then(|s| s.parse::<u128>().ok())
            .and_then(|ns| {
                let prev_entry = prev.get(&name)?;
                let dns = ns.saturating_sub(prev_entry.0) as f64;
                let ds = now.duration_since(prev_entry.1).as_secs_f64();
                if ds > 0.0 {
                    Some(dns / 1e9 / ds * 100.0)
                } else {
                    None
                }
            });
        if let Some(ns) = d
            .and_then(|m| m.get("cpu.time"))
            .and_then(|s| s.parse::<u128>().ok())
        {
            prev.insert(name.clone(), (ns, now));
        }
        let kib = |key: &str| {
            d.and_then(|m| m.get(key))
                .and_then(|s| s.parse::<u64>().ok())
                .map(|k| k.saturating_mul(1024))
        };
        out.push(VmRow {
            name,
            status: state,
            engine: "virsh".to_string(),
            engine_version: ver.clone(),
            cpu_count: d
                .and_then(|m| m.get("vcpu.current"))
                .and_then(|s| s.parse::<u64>().ok()),
            cpu_percent,
            memory_usage: kib("balloon.current"),
            memory_total: kib("balloon.maximum"),
            ipv4: None,
        });
    }
    prev.retain(|name, _| out.iter().any(|r| &r.name == name));
    out
}

/// Collect Multipass instances.
pub fn collect_multipass(version: &OnceLock<String>) -> Vec<VmRow> {
    let bin = match find_bin(
        &["/snap/bin", "/usr/bin", "/bin", "/usr/local/bin"],
        "multipass",
    ) {
        Some(b) => b,
        None => return Vec::new(),
    };
    let list = match run(&bin, &["list", "--format", "csv"]) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let ver = version
        .get_or_init(|| {
            run(&bin, &["version"])
                .and_then(|s| {
                    s.lines()
                        .next()
                        .map(|l| l.trim().trim_start_matches("multipass").trim().to_string())
                })
                .unwrap_or_default()
        })
        .clone();
    parse_multipass_csv(&list)
        .into_iter()
        .map(|(name, state, ipv4, release)| VmRow {
            name,
            status: state,
            engine: "multipass".to_string(),
            engine_version: ver.clone(),
            cpu_count: None,
            cpu_percent: None,
            memory_usage: None,
            memory_total: None,
            ipv4: if ipv4 == "--" || ipv4.is_empty() {
                None
            } else {
                Some(format!("{} ({})", ipv4, release))
            },
        })
        .collect()
}

/// Render one VM row as a stats object. Missing engine fields are omitted.
pub fn row_to_value(r: &VmRow) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("name".into(), Value::String(r.name.clone()));
    obj.insert("status".into(), Value::String(r.status.clone()));
    obj.insert("engine".into(), Value::String(r.engine.clone()));
    obj.insert(
        "engine_version".into(),
        Value::String(r.engine_version.clone()),
    );
    if let Some(c) = r.cpu_count {
        obj.insert("cpu_count".into(), Value::Uint(c));
    }
    if let Some(p) = r.cpu_percent {
        obj.insert("cpu_time".into(), Value::Float((p * 100.0).round() / 100.0));
    }
    if let Some(u) = r.memory_usage {
        obj.insert("memory_usage".into(), Value::Uint(u));
    }
    if let Some(t) = r.memory_total {
        obj.insert("memory_total".into(), Value::Uint(t));
    }
    if let Some(ip) = &r.ipv4 {
        obj.insert("ipv4".into(), Value::String(ip.clone()));
    }
    Value::Object(obj)
}

impl Plugin for VmsPlugin {
    fn name(&self) -> &'static str {
        NAME
    }
    fn reset(&mut self) {
        self.base.reset();
        self.cached.clear();
        self.collected_at = None;
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
    fn history_items(&self) -> &[&'static str] { &["memory_usage"] }
    fn get_key(&self) -> Option<&'static str> {
        Some("name")
    }

    fn update(&mut self) -> Result<()> {
        if !cfg!(target_os = "linux") {
            self.base.stats = Value::Array(Vec::new());
            return Ok(());
        }
        let now = Instant::now();
        if !crate::plugins::smart::cache_fresh(self.collected_at, now) {
            let mut rows = collect_virsh(&mut self.prev_cpu, &self.virsh_version);
            rows.extend(collect_multipass(&self.multipass_version));
            self.cached = rows;
            self.collected_at = Some(now);
        }
        self.base.stats = Value::Array(self.cached.iter().map(row_to_value).collect());
        Ok(())
    }
}
