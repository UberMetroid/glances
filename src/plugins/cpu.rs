//! CPU plugin — aggregate + per-state CPU% via /proc/stat.
//!
//! Mirrors `glances/plugins/cpu/__init__.py`. Two reads are needed to
//! produce a percentage: this tick and the previous tick. The first
//! update emits zeros for percentage fields.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::platform as plat;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "cpu";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(CpuPlugin::new()));
}

pub struct CpuPlugin {
    base: GlancesPluginModel,
    prev: Option<plat::linux::proc_stat::CpuTimes>,
    prev_ctx: Option<u64>,
    prev_intr: Option<u64>,
}

impl CpuPlugin {
    pub fn new() -> Self {
        let mut stats = BTreeMap::new();
        stats.insert("total".into(), Value::Float(0.0));
        stats.insert("user".into(), Value::Float(0.0));
        stats.insert("system".into(), Value::Float(0.0));
        stats.insert("idle".into(), Value::Float(0.0));
        stats.insert("iowait".into(), Value::Float(0.0));
        stats.insert("irq".into(), Value::Float(0.0));
        stats.insert("nice".into(), Value::Float(0.0));
        stats.insert("steal".into(), Value::Float(0.0));
        stats.insert("guest".into(), Value::Float(0.0));
        stats.insert("ctx_switches".into(), Value::Float(0.0));
        stats.insert("interrupts".into(), Value::Float(0.0));
        let stats_init = Value::Object(stats);
        Self {
            base: GlancesPluginModel::new(NAME, stats_init),
            prev: None,
            prev_ctx: None,
            prev_intr: None,
        }
    }
}

/// Fill per-state percentage fields from a tick-over-tick delta.
/// Each state's share = (field delta) / (total delta) * 100 — the same
/// math `top`/`htop` and Python Glances use.
pub fn state_pcts(cur: &mut BTreeMap<String, Value>, d: &plat::linux::proc_stat::CpuTimes) {
    let dt = d.total() as f64;
    if dt <= 0.0 { return; }
    let pct = |v: u64| v as f64 / dt * 100.0;
    for (k, v) in [
        ("user", d.user), ("nice", d.nice), ("system", d.system),
        ("idle", d.idle), ("iowait", d.iowait), ("irq", d.irq),
        ("steal", d.steal), ("guest", d.guest),
    ] {
        cur.insert(k.into(), Value::Float(pct(v)));
    }
    let busy = d.busy() as f64 / dt * 100.0;
    cur.insert("total".into(), Value::Float(busy.max(0.0).min(100.0)));
}

impl Plugin for CpuPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        let proc = plat::linux::proc_stat::read()?;

        let mut cur = self.base.stats.as_object().cloned().unwrap_or_default();
        if let Some(prev) = &self.prev {
            state_pcts(&mut cur, &proc.total.delta(prev));
            if let Some(pctx) = self.prev_ctx {
                let dctx = proc.ctxt.saturating_sub(pctx);
                cur.insert("ctx_switches".into(), Value::Float(dctx as f64));
            }
            if let Some(pintr) = self.prev_intr {
                let dintr = proc.intr.saturating_sub(pintr);
                cur.insert("interrupts".into(), Value::Float(dintr as f64));
            }
        }
        self.base.stats = Value::Object(cur);
        self.prev = Some(proc.total);
        self.prev_ctx = Some(proc.ctxt);
        self.prev_intr = Some(proc.intr);
        Ok(())
    }
}
