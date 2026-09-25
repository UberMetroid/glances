//! Per-CPU stats — one row per logical CPU from /proc/stat.
//!
//! Rows carry the aggregate field set scoped to one CPU, keyed by
//! `cpu_number` (`cpu<N>`) for views and history. Percentages need a
//! previous sample per CPU; first ticks and hotplugged CPUs report
//! zeros with a stable schema.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "percpu";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(PerCpuPlugin::new()));
}

pub struct PerCpuPlugin {
    base: GlancesPluginModel,
    prev: Option<Vec<plat::linux::proc_stat::CpuTimes>>,
}

impl Default for PerCpuPlugin {
    fn default() -> Self { Self::new() }
}

impl PerCpuPlugin {
    pub fn new() -> Self {
        // The CPU count is unknown until the first read.
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())), prev: None }
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
            m.insert("key".into(), Value::String("cpu_number".into()));
            m.insert("cpu_number".into(), Value::String(format!("cpu{idx}")));
            for k in ["user", "nice", "system", "idle", "iowait", "irq", "steal", "guest", "total"] {
                m.insert(k.into(), Value::Float(0.0));
            }
            let d = self
                .prev
                .as_ref()
                .and_then(|rows| rows.get(idx))
                .map(|p| t.delta(p))
                .unwrap_or_default();
            super::cpu::state_pcts(&mut m, &d);
            let dt = d.total() as f64;
            if dt > 0.0 {
                m.insert("softirq".into(), Value::Float(d.softirq as f64 / dt * 100.0));
                let idle = d.idle as f64 / dt * 100.0;
                // Per-CPU total is 100-idle (iowait and steal count as
                // used here, unlike the aggregate's busy share).
                m.insert("total".into(), Value::Float((100.0 - idle).clamp(0.0, 100.0)));
                m.insert("busy".into(), Value::Float(d.busy() as f64 / dt * 100.0));
            } else {
                m.insert("softirq".into(), Value::Float(0.0));
                m.insert("busy".into(), Value::Float(0.0));
            }
            out.push(Value::Object(m));
        }
        self.base.stats = Value::Array(out);
        self.prev = Some(proc.per_cpu);
        Ok(())
    }
}
