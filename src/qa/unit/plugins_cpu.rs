//! Tests for the aggregate cpu plugin — delta-based percentages.
//!
//! Regression coverage for the bug where cumulative-since-boot jiffies
//! were divided by a per-tick delta and fields were shuffled
//! (user←system, idle←iowait, ...), producing values like 499671%.

use std::collections::BTreeMap;

use crate::core::plugin::Plugin;
use crate::core::value::Value;
use crate::platform::linux::proc_stat::CpuTimes;
use crate::plugins::cpu::{self, state_pcts, CpuPlugin};

fn times(u: u64, n: u64, s: u64, i: u64) -> CpuTimes {
    CpuTimes { user: u, nice: n, system: s, idle: i, ..Default::default() }
}

#[test]
fn state_pcts_uses_delta_fields() {
    // delta: user+100, system+100, idle+200 → total delta 400.
    let d = times(100, 0, 100, 200);
    let mut m = BTreeMap::new();
    state_pcts(&mut m, &d);
    let get = |k: &str| m.get(k).and_then(Value::as_f64).unwrap();
    assert_eq!(get("user"), 25.0);
    assert_eq!(get("system"), 25.0);
    assert_eq!(get("idle"), 50.0);
    // Fields the old code shuffled or zeroed must track their own deltas.
    assert_eq!(get("iowait"), 0.0);
    assert_eq!(get("irq"), 0.0);
    assert_eq!(get("nice"), 0.0);
    assert_eq!(get("steal"), 0.0);
}

#[test]
fn state_pcts_every_state_tracks_own_delta() {
    let mut d = CpuTimes::default();
    d.user = 10; d.nice = 10; d.system = 10; d.idle = 10;
    d.iowait = 10; d.irq = 10; d.softirq = 10; d.steal = 10;
    // total = busy(70) + idle(10) = 80 → each listed state = 12.5%.
    let mut m = BTreeMap::new();
    state_pcts(&mut m, &d);
    let get = |k: &str| m.get(k).and_then(Value::as_f64).unwrap();
    for k in ["user", "nice", "system", "idle", "iowait", "irq", "steal"] {
        assert_eq!(get(k), 12.5, "{} must be 12.5%", k);
    }
}

#[test]
fn state_pcts_zero_delta_leaves_fields() {
    let mut m = BTreeMap::new();
    m.insert("user".into(), Value::Float(7.0));
    state_pcts(&mut m, &CpuTimes::default());
    assert_eq!(m.get("user").and_then(Value::as_f64), Some(7.0));
}

#[test]
fn plugin_first_tick_is_zero_and_second_is_bounded() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = CpuPlugin::new();
    p.update().expect("update 1");
    let m = p.stats().as_object().unwrap();
    assert_eq!(m.get("total").and_then(Value::as_f64), Some(0.0),
        "first tick must not emit percentages");
    p.update().expect("update 2");
    let m = p.stats().as_object().unwrap();
    for k in ["total", "user", "system", "idle", "iowait"] {
        let v = m.get(k).and_then(Value::as_f64).unwrap_or(-1.0);
        assert!(v >= 0.0 && v <= 100.0, "{} out of range: {}", k, v);
    }
}

#[test]
fn plugin_registers() {
    let s = crate::core::stats::GlancesStats::new(1.0);
    cpu::register(&s);
    assert!(s.plugin_names().contains(&"cpu"));
}
