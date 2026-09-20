//! QA tests for GlancesStats registry + post-update aggregation.

use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins;

fn f64_at(v: &Value, plugin: &str, key: &str) -> f64 {
    v.as_object()
        .and_then(|o| o.get(plugin))
        .and_then(|p| p.as_object())
        .and_then(|p| p.get(key))
        .and_then(Value::as_f64)
        .unwrap_or(f64::NAN)
}

#[test]
fn quicklook_aggregation_populates_summary() {
    let stats = GlancesStats::new(1.0);
    plugins::register_all(&stats);
    // Two ticks: the first primes delta counters (cpu rates are 0 on
    // tick 1), the second produces live values.
    stats.update().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    stats.update().unwrap();
    let snap = stats.snapshot();
    let ql = snap.as_object().and_then(|o| o.get("quicklook")).expect("quicklook stats");
    let obj = ql.as_object().expect("quicklook is an object");
    for k in ["cpu", "mem", "swap", "load", "cpu_name"] {
        assert!(obj.contains_key(k), "quicklook missing {k}");
    }
    // mem comes straight from the mem plugin — must be a real percent.
    let mem_pct = f64_at(&snap, "mem", "percent");
    assert_eq!(f64_at(&snap, "quicklook", "mem"), mem_pct);
    // load = min1 / cpucore * 100.
    let min1 = f64_at(&snap, "load", "min1");
    let cores = f64_at(&snap, "load", "cpucore");
    if cores > 0.0 {
        let expect = min1 / cores * 100.0;
        assert!((f64_at(&snap, "quicklook", "load") - expect).abs() < 1e-6);
    }
    // cpu_name is a non-empty string on real hardware.
    if let Some(Value::String(n)) = obj.get("cpu_name") {
        assert!(!n.is_empty());
    }
}

#[test]
fn update_records_numeric_history_by_default() {
    use std::sync::atomic::Ordering;
    let stats = GlancesStats::new(1.0);
    plugins::register_all(&stats);
    assert!(stats.history_enabled.load(Ordering::Relaxed));
    stats.update().unwrap();
    // mem.percent is a top-level numeric — must be recorded.
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mem = guard.iter().find(|p| p.name() == "mem").expect("mem plugin");
    let model = mem.model().expect("mem model");
    assert!(!model.stats_history.snapshot().is_empty(), "history must record");
    assert!(!model.stats_history.get("percent", 0).is_empty());
}

#[test]
fn disable_history_stops_recording() {
    use std::sync::atomic::Ordering;
    let stats = GlancesStats::new(1.0);
    stats.history_enabled.store(false, Ordering::Relaxed);
    plugins::register_all(&stats);
    stats.update().unwrap();
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    for p in guard.iter() {
        if let Some(model) = p.model() {
            assert!(model.stats_history.snapshot().is_empty(), "{} recorded", p.name());
        }
    }
}

#[test]
fn history_records_curated_element_series() {
    // Upstream parity: list plugins record `<elem>_<field>` series
    // (e.g. `eth0_bytes_recv_rate_per_sec`), capped at history_size.
    use crate::core::value::Value;
    use std::collections::BTreeMap;
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_all(&stats);
    let mut nic = BTreeMap::new();
    nic.insert("interface_name".into(), Value::String("eth0".into()));
    nic.insert("bytes_recv_rate_per_sec".into(), Value::Float(7.0));
    nic.insert("bytes_sent_rate_per_sec".into(), Value::Float(8.0));
    let mut guard = stats.plugins.write().unwrap();
    let p = guard.iter_mut().find(|p| p.name() == "network").expect("network");
    let m = p.model_mut().expect("model");
    m.stats = Value::Array(vec![Value::Object(nic)]);
    m.update_stats_history(&["bytes_recv_rate_per_sec", "bytes_sent_rate_per_sec"], Some("interface_name"), 3);
    let vals: Vec<f64> = m.stats_history.get("eth0_bytes_recv_rate_per_sec", 0).iter().map(|s| s.value).collect();
    assert_eq!(vals, vec![7.0]);
    // Cap honored: push past history_size and check truncation.
    for i in 0..5 {
        m.stats_history.add("eth0_bytes_recv_rate_per_sec", i as f64);
    }
    assert!(m.stats_history.get("eth0_bytes_recv_rate_per_sec", 0).len() <= 3);
}
