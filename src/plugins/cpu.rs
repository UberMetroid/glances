//! CPU plugin — aggregate state percentages plus counters.
//!
//! Percentages need two samples (this tick vs the last), so the first
//! update emits zeros. `ctx_switches`/`interrupts`/`soft_interrupts`
//! are cumulative since-boot counters with `*_rate_per_sec` siblings.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{FieldDesc, FieldFlags, GlancesPluginModel, Plugin, Unit};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "cpu";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(CpuPlugin::new()));
}

/// Previous sample of one cumulative counter, for per-second rates.
#[derive(Default, Clone)]
struct RateTrack {
    prev: Option<(u64, std::time::Instant)>,
}

impl RateTrack {
    /// Record `cur`, returning the rate since the previous call (0.0
    /// on the first call or a zero-length gap).
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

impl Default for CpuPlugin {
    fn default() -> Self { Self::new() }
}

impl CpuPlugin {
    pub fn new() -> Self {
        let mut stats = BTreeMap::new();
        for k in [
            "total", "user", "system", "idle", "iowait", "dpc", "irq", "nice", "steal", "guest",
            "ctx_switches", "ctx_switches_rate_per_sec",
            "interrupts", "interrupts_rate_per_sec",
            "soft_interrupts", "soft_interrupts_rate_per_sec",
            "syscalls", "cpucore", "time_since_update",
        ] {
            stats.insert(k.into(), Value::Float(0.0));
        }
        // Identity never changes between ticks — read once here.
        stats.insert(
            "cpu_name".into(),
            match plat::linux::proc_cpuinfo::model_name() {
                Some(n) => Value::String(n),
                None => Value::Null,
            },
        );
        stats.insert("cpucore".into(), Value::Float(plat::linux::proc_cpuinfo::cpu_count() as f64));
        Self {
            base: GlancesPluginModel::new(NAME, Value::Object(stats)),
            prev: None,
            prev_at: None,
            ctx: RateTrack::default(),
            intr: RateTrack::default(),
            softirq: RateTrack::default(),
        }
    }
}

/// Per-state shares from one tick delta: each field's delta over the
/// total delta ×100 (the same math as `top`). A zero total (counter
/// reset) leaves every field untouched.
pub fn state_pcts(cur: &mut BTreeMap<String, Value>, d: &plat::linux::proc_stat::CpuTimes) {
    let dt = d.total() as f64;
    if dt <= 0.0 {
        return;
    }
    for (k, v) in [
        ("user", d.user),
        ("nice", d.nice),
        ("system", d.system),
        ("idle", d.idle),
        ("iowait", d.iowait),
        ("irq", d.irq),
        ("steal", d.steal),
        ("guest", d.guest),
    ] {
        cur.insert(k.into(), Value::Float(v as f64 / dt * 100.0));
    }
    cur.insert("total".into(), Value::Float((d.busy() as f64 / dt * 100.0).clamp(0.0, 100.0)));
}

impl Plugin for CpuPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["user", "system"] }
    fn update_snmp(&mut self, ctx: &crate::core::snmp::SnmpCtx) -> Result<()> {
        // UCD percentages by default; windows/esxi average the
        // hrProcessorLoad table instead.
        let (user, system, idle) = match ctx.system_name.as_deref() {
            Some("windows") | Some("esxi") => {
                let rows =
                    ctx.client.walk("1.3.6.1.2.1.25.3.3.1.2", 256).unwrap_or_default();
                let vals: Vec<f64> = rows.iter().filter_map(|(_, v)| v.as_f64()).collect();
                if vals.is_empty() {
                    self.reset();
                    return Ok(());
                }
                let total = vals.iter().sum::<f64>() / vals.len() as f64;
                (total, 0.0, 100.0 - total)
            }
            _ => {
                let m = crate::core::snmp::get_map(&ctx.client, &[
                    ("user", "1.3.6.1.4.1.2021.11.9.0"),
                    ("system", "1.3.6.1.4.1.2021.11.10.0"),
                    ("idle", "1.3.6.1.4.1.2021.11.11.0"),
                ])?;
                let f = |k: &str| m.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
                (f("user"), f("system"), f("idle"))
            }
        };
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("user".into(), Value::Float(user));
            obj.insert("system".into(), Value::Float(system));
            obj.insert("idle".into(), Value::Float(idle.clamp(0.0, 100.0)));
            obj.insert("total".into(), Value::Float((100.0 - idle).clamp(0.0, 100.0)));
        }
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        let proc = plat::linux::proc_stat::read()?;
        let now = std::time::Instant::now();
        // Seconds since the previous tick (0.0 on the first).
        let since = self.prev_at.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        let mut cur = self.base.stats.as_object().cloned().unwrap_or_default();
        if let Some(prev) = &self.prev {
            state_pcts(&mut cur, &proc.total.delta(prev));
        }
        // Live fields pinned to 0.0 here: `syscalls` is always 0 on
        // Linux and `dpc` is Windows-only.
        cur.insert("syscalls".into(), Value::Float(0.0));
        cur.insert("dpc".into(), Value::Float(0.0));
        cur.insert("time_since_update".into(), Value::Float(since.max(0.0)));
        cur.insert("ctx_switches".into(), Value::Float(proc.ctxt as f64));
        cur.insert(
            "ctx_switches_rate_per_sec".into(),
            Value::Float(self.ctx.sample(proc.ctxt)),
        );
        cur.insert("interrupts".into(), Value::Float(proc.intr as f64));
        cur.insert(
            "interrupts_rate_per_sec".into(),
            Value::Float(self.intr.sample(proc.intr)),
        );
        cur.insert("soft_interrupts".into(), Value::Float(proc.softirq_total as f64));
        cur.insert(
            "soft_interrupts_rate_per_sec".into(),
            Value::Float(self.softirq.sample(proc.softirq_total)),
        );
        self.base.stats = Value::Object(cur);
        self.prev = Some(proc.total);
        self.prev_at = Some(now);
        Ok(())
    }
    fn fields_description(&self) -> &[FieldDesc] { CPU_DESCS }
    fn update_views(&mut self, events: &mut EventLog) {
        let Some(m) = self.model_mut() else { return };
        m.build_views(CPU_DESCS, None, Some(&mut *events));
        // ctx_switches classifies against 100×cores once a tick gap
        // exists (the first tick has no timespan to judge).
        let since = m.stats.as_object().and_then(|o| o.get("time_since_update")).and_then(Value::as_f64);
        if since.is_none_or(|s| s == 0.0) {
            return;
        }
        let cores = m
            .stats
            .as_object()
            .and_then(|o| o.get("cpucore"))
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .max(1.0);
        let cur = m.stats.as_object().and_then(|o| o.get("ctx_switches")).and_then(Value::as_f64).unwrap_or(0.0);
        let d = m.get_alert(cur, 0.0, 100.0 * cores, "ctx_switches", None, false, false, None, Some(&mut *events));
        m.views.entry(String::new()).or_default().insert("ctx_switches".into(), d);
    }
}

/// Percent states log; steal alerts; ctx_switches handled above.
const CPU_DESCS: &[FieldDesc] = &[
    FieldDesc { name: "total", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "user", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "system", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "iowait", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "dpc", unit: Unit::Percent, flags: FieldFlags::LOG },
    FieldDesc { name: "steal", unit: Unit::Percent, flags: FieldFlags::ALERT },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shares_sum_over_the_delta() {
        let d = plat::linux::proc_stat::CpuTimes {
            user: 50, nice: 0, system: 25, idle: 25, iowait: 0, irq: 0,
            softirq: 0, steal: 0, guest: 0, guest_nice: 0,
        };
        let mut m = BTreeMap::new();
        state_pcts(&mut m, &d);
        assert_eq!(m["user"].as_f64(), Some(50.0));
        assert_eq!(m["system"].as_f64(), Some(25.0));
        assert_eq!(m["idle"].as_f64(), Some(25.0));
        let total = m["total"].as_f64().unwrap();
        assert!((0.0..=100.0).contains(&total));
    }
    #[test]
    fn zero_delta_leaves_fields_untouched() {
        let d = plat::linux::proc_stat::CpuTimes::default();
        let mut m = BTreeMap::from([("user".to_string(), Value::Float(7.0))]);
        state_pcts(&mut m, &d);
        assert_eq!(m["user"].as_f64(), Some(7.0));
        assert!(!m.contains_key("total"));
    }
    #[test]
    fn counter_rates_need_two_samples() {
        let mut r = RateTrack::default();
        assert_eq!(r.sample(100), 0.0);
        assert!(r.sample(100) >= 0.0);
    }
}
