//! CPU plugin — aggregate + per-state CPU% via /proc/stat.
//!
//! Mirrors `glances/plugins/cpu/__init__.py`. Two reads are needed to
//! produce a percentage: this tick and the previous tick. The first
//! update emits zeros for percentage fields.
//!
//! `ctx_switches`/`interrupts`/`soft_interrupts` are **cumulative**
//! since-boot counters (psutil `cpu_stats()` parity) with
//! `<key>_rate_per_sec` siblings computed over wall-clock time.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::platform as plat;
use crate::core::events::EventLog;
use crate::core::plugin::{FieldDesc, FieldFlags, GlancesPluginModel, Plugin, Unit};
use crate::core::value::Value;

pub const NAME: &str = "cpu";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(CpuPlugin::new()));
}

/// Cumulative counter + its previous sample, for `*_rate_per_sec`.
#[derive(Default, Clone)]
struct RateTrack {
    prev: Option<(u64, std::time::Instant)>,
}

impl RateTrack {
    /// Store `cur`, returning the per-second rate vs the previous call.
    fn sample(&mut self, cur: u64) -> f64 {
        let now = std::time::Instant::now();
        let rate = match self.prev {
            Some((p, t)) => {
                let dt = now.duration_since(t).as_secs_f64();
                if dt > 0.0 { cur.saturating_sub(p) as f64 / dt } else { 0.0 }
            }
            None => 0.0,
        };
        self.prev = Some((cur, now));
        rate
    }
}

pub struct CpuPlugin {
    base: GlancesPluginModel,
    prev: Option<plat::linux::proc_stat::CpuTimes>,
    prev_at: Option<std::time::Instant>,
    ctx: RateTrack,
    intr: RateTrack,
    softirq: RateTrack,
}

impl CpuPlugin {
    pub fn new() -> Self {
        let mut stats = BTreeMap::new();
        for k in [
            "total", "user", "system", "idle", "iowait", "dpc", "irq", "nice",
            "steal", "guest",
            "ctx_switches", "ctx_switches_rate_per_sec",
            "interrupts", "interrupts_rate_per_sec",
            "soft_interrupts", "soft_interrupts_rate_per_sec",
            "syscalls", "cpucore", "time_since_update",
        ] {
            stats.insert(k.into(), Value::Float(0.0));
        }
        // Static fields that never change between ticks; populated once.
        stats.insert("cpu_name".into(), match plat::linux::proc_cpuinfo::model_name() {
            Some(n) => Value::String(n),
            None => Value::Null,
        });
        stats.insert("cpucore".into(),
            Value::Float(plat::linux::proc_cpuinfo::cpu_count() as f64));
        let stats_init = Value::Object(stats);
        Self {
            base: GlancesPluginModel::new(NAME, stats_init),
            prev: None,
            prev_at: None,
            ctx: RateTrack::default(),
            intr: RateTrack::default(),
            softirq: RateTrack::default(),
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
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["user", "system"] }
    fn update(&mut self) -> Result<()> {
        let proc = plat::linux::proc_stat::read()?;
        let now = std::time::Instant::now();
        // Upstream `time_since_update`: seconds since the previous tick
        // (0.0 on the first tick).
        let since = self.prev_at.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);

        let mut cur = self.base.stats.as_object().cloned().unwrap_or_default();
        if let Some(prev) = &self.prev {
            state_pcts(&mut cur, &proc.total.delta(prev));
        }
        // Upstream parity: `syscalls` is always 0 on Linux and `dpc` is
        // Windows-only — both are live fields pinned to 0.0 here.
        cur.insert("syscalls".into(), Value::Float(0.0));
        cur.insert("dpc".into(), Value::Float(0.0));
        cur.insert("time_since_update".into(), Value::Float(since.max(0.0)));
        // Cumulative counters + rate siblings (psutil cpu_stats parity).
        cur.insert("ctx_switches".into(), Value::Float(proc.ctxt as f64));
        cur.insert("ctx_switches_rate_per_sec".into(),
            Value::Float(self.ctx.sample(proc.ctxt)));
        cur.insert("interrupts".into(), Value::Float(proc.intr as f64));
        cur.insert("interrupts_rate_per_sec".into(),
            Value::Float(self.intr.sample(proc.intr)));
        cur.insert("soft_interrupts".into(), Value::Float(proc.softirq_total as f64));
        cur.insert("soft_interrupts_rate_per_sec".into(),
            Value::Float(self.softirq.sample(proc.softirq_total)));
        self.base.stats = Value::Object(cur);
        self.prev = Some(proc.total);
        self.prev_at = Some(now);
        Ok(())
    }
    fn fields_description(&self) -> &[FieldDesc] { CPU_DESCS }
    fn update_views(&mut self, events: &mut EventLog) {
        if let Some(m) = self.model_mut() {
            m.build_views(CPU_DESCS, None, Some(&mut *events));
            // ctx_switches alert (upstream cpu update_views): skipped
            // without a timespan, maximum scales with core count.
            let since = m.stats.as_object()
                .and_then(|o| o.get("time_since_update"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            if since == 0.0 { return; }
            let cores = m.stats.as_object()
                .and_then(|o| o.get("cpucore"))
                .and_then(Value::as_f64)
                .unwrap_or(1.0)
                .max(1.0);
            let cur = m.stats.as_object()
                .and_then(|o| o.get("ctx_switches"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let d = m.get_alert(cur, 0.0, 100.0 * cores, "ctx_switches", None, false, false, None, Some(&mut *events));
            m.views.entry(String::new()).or_default().insert("ctx_switches".into(), d);
        }
    }
}

/// Upstream cpu `fields_description` alert wiring: percent states log,
/// steal alerts (unit percent throughout).
const CPU_DESCS: &[FieldDesc] = &[
    FieldDesc { name: "total", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "user", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "system", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "iowait", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "dpc", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "steal", unit: Unit::Percent, flags: FieldFlags::ALERT },
];
