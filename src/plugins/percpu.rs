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

pub struct PerCpuPlugin { base: GlancesPluginModel }

impl PerCpuPlugin {
    pub fn new() -> Self {
        // Empty array — we don't know how many CPUs we have at construction.
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

impl Plugin for PerCpuPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn get_key(&self) -> Option<&'static str> { Some("cpu_number") }

    fn update(&mut self) -> Result<()> {
        let proc = plat::linux::proc_stat::read()?;
        let mut out: Vec<Value> = Vec::with_capacity(proc.per_cpu.len());
        for (idx, t) in proc.per_cpu.iter().enumerate() {
            let mut m: BTreeMap<String, Value> = BTreeMap::new();
            m.insert("cpu_number".into(), Value::String(format!("cpu{}", idx)));
            m.insert("user".into(), Value::Float(t.user as f64));
            m.insert("system".into(), Value::Float(t.system as f64));
            m.insert("idle".into(), Value::Float(t.idle as f64));
            m.insert("iowait".into(), Value::Float(t.iowait as f64));
            m.insert("nice".into(), Value::Float(t.nice as f64));
            m.insert("irq".into(), Value::Float(t.irq as f64));
            m.insert("softirq".into(), Value::Float(t.softirq as f64));
            m.insert("steal".into(), Value::Float(t.steal as f64));
            m.insert("guest".into(), Value::Float(t.guest as f64));
            m.insert("total".into(), Value::Float(t.total() as f64));
            m.insert("busy".into(), Value::Float(t.busy() as f64));
            out.push(Value::Object(m));
        }
        self.base.stats = Value::Array(out);
        Ok(())
    }
}