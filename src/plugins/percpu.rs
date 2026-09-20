//! Per-CPU stats — one entry per logical CPU from /proc/stat.
//!
//! Mirrors `glances/plugins/percpu/__init__.py`. Each entry has all the
//! fields of the aggregate `cpu` plugin but scoped to a single CPU, plus
//! a `cpu_number` key used to identify the row in the array.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::platform as plat;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "percpu";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(PerCpuPlugin::new()));
}

pub struct PerCpuPlugin {
    base: GlancesPluginModel,
    prev: Option<Vec<plat::linux::proc_stat::CpuTimes>>,
}

impl PerCpuPlugin {
    pub fn new() -> Self {
        // Empty array — we don't know how many CPUs we have at construction.
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            prev: None,
        }
    }
}

impl Plugin for PerCpuPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["user", "system"] }
    fn get_key(&self) -> Option<&'static str> { Some("cpu_number") }

    fn update(&mut self) -> Result<()> {
        let proc = plat::linux::proc_stat::read()?;
        let mut out: Vec<Value> = Vec::with_capacity(proc.per_cpu.len());
        for (idx, t) in proc.per_cpu.iter().enumerate() {
            let mut m: BTreeMap<String, Value> = BTreeMap::new();
            m.insert("cpu_number".into(), Value::String(format!("cpu{}", idx)));
            // Percentages need a previous sample for this CPU. First tick
            // and hotplugged CPUs report 0.0 (stable schema every tick).
            for k in ["user","nice","system","idle","iowait","irq","steal","guest","total"] {
                m.insert(k.into(), Value::Float(0.0));
            }
            let d = self.prev
                .as_ref()
                .and_then(|rows| rows.get(idx))
                .map(|p| t.delta(p))
                .unwrap_or_default();
            super::cpu::state_pcts(&mut m, &d);
            let dt = d.total() as f64;
            let softirq = if dt > 0.0 { d.softirq as f64 / dt * 100.0 } else { 0.0 };
            m.insert("softirq".into(), Value::Float(softirq));
            // Python percpu parity: `total` = 100 - idle (NOT busy/total
            // like the aggregate plugin — iowait+steal count as used).
            // dt==0 (first tick / counter reset) → 0.0, not 100.
            let idle = if dt > 0.0 { d.idle as f64 / dt * 100.0 } else { 0.0 };
            let total = if dt > 0.0 { (100.0 - idle).max(0.0).min(100.0) } else { 0.0 };
            m.insert("total".into(), Value::Float(total));
            m.insert("busy".into(), Value::Float(
                if dt > 0.0 { d.busy() as f64 / dt * 100.0 } else { 0.0 }));
            out.push(Value::Object(m));
        }
        self.base.stats = Value::Array(out);
        self.prev = Some(proc.per_cpu);
        Ok(())
    }
}