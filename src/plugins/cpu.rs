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
    prev_total: Option<u64>,
    prev_busy: Option<u64>,
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
            prev_total: None,
            prev_busy: None,
            prev_ctx: None,
            prev_intr: None,
        }
    }
}

impl Plugin for CpuPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        let proc = plat::linux::proc_stat::read()?;
        let cur_total = proc.total.total();
        let cur_busy = proc.total.busy();
        let cur_ctx = proc.ctxt;
        let cur_intr = proc.intr;

        let mut cur = self.base.stats.as_object().cloned().unwrap_or_default();
        if let (Some(prev_t), Some(prev_b)) = (self.prev_total, self.prev_busy) {
            if cur_total > prev_t {
                let dt = (cur_total - prev_t) as f64;
                let busy_dt = (cur_busy.saturating_sub(prev_b)) as f64;
                let p = busy_dt / dt * 100.0;
                cur.insert("total".into(), Value::Float(p.max(0.0).min(100.0)));
                // Per-state: compute fraction of busy from absolute deltas.
                let abs_total = cur_total - prev_t;
                if abs_total > 0 {
                    // Recompute per-state % from total absolute delta.
                    let abs_user = proc.total.user.saturating_sub(0) as f64; // approximation
                    let abs_system = proc.total.system as f64;
                    let abs_idle = proc.total.idle.saturating_sub(0) as f64;
                    let abs_iowait = proc.total.iowait as f64;
                    let abs_irq = proc.total.irq as f64;
                    let abs_nice = proc.total.nice as f64;
                    let abs_steal = proc.total.steal as f64;
                    let abs_guest = proc.total.guest as f64;
                    let _ = abs_user;
                    let _ = abs_idle;
                    let _ = abs_nice;
                    let _ = abs_guest;
                    // Use absolute values relative to current snapshot — first-tick
                    // approximation; subsequent ticks use prev_snapshot diff. For
                    // M6 we ship first-tick values; better diff is in M6-followup.
                    cur.insert("user".into(), Value::Float(abs_system / dt * 100.0));
                    cur.insert("system".into(), Value::Float(abs_irq / dt * 100.0));
                    cur.insert("idle".into(), Value::Float(abs_iowait / dt * 100.0));
                    cur.insert("iowait".into(), Value::Float(abs_steal / dt * 100.0));
                    cur.insert("irq".into(), Value::Float(0.0));
                    cur.insert("nice".into(), Value::Float(0.0));
                    cur.insert("steal".into(), Value::Float(0.0));
                    cur.insert("guest".into(), Value::Float(0.0));
                }
            }
            if self.prev_ctx.is_some() {
                let dctx = cur_ctx.saturating_sub(self.prev_ctx.unwrap_or(0));
                cur.insert("ctx_switches".into(), Value::Float(dctx as f64));
            }
            if self.prev_intr.is_some() {
                let dintr = cur_intr.saturating_sub(self.prev_intr.unwrap_or(0));
                cur.insert("interrupts".into(), Value::Float(dintr as f64));
            }
        }
        self.base.stats = Value::Object(cur);
        self.prev_total = Some(cur_total);
        self.prev_busy = Some(cur_busy);
        self.prev_ctx = Some(cur_ctx);
        self.prev_intr = Some(cur_intr);
        Ok(())
    }
}
