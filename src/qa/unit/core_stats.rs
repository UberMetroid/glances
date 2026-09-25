//! Registry behavior: quicklook summaries, default history recording,
//! and the history kill-switch — all against live registered plugins.

use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins;

fn number(snap: &Value, plugin: &str, key: &str) -> f64 {
    snap.as_object()
        .and_then(|o| o.get(plugin))
        .and_then(|p| p.as_object())
        .and_then(|p| p.get(key))
        .and_then(Value::as_f64)
        .unwrap_or(f64::NAN)
}

fn live() -> GlancesStats {
    let stats = GlancesStats::new(1.0);
    plugins::register_all(&stats);
    stats.update().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    stats.update().unwrap();
    stats
}

#[test]
fn quicklook_carries_cpu_mem_swap_load_and_name() {
    let snap = live().snapshot();
    let obj = snap
        .as_object().and_then(|o| o.get("quicklook")).and_then(|q| q.as_object())
        .expect("quicklook object");
    for k in ["cpu", "mem", "swap", "load", "cpu_name"] {
        assert!(obj.contains_key(k), "quicklook missing {k}");
    }
    assert_eq!(number(&snap, "quicklook", "mem"), number(&snap, "mem", "percent"));
    let cores = number(&snap, "load", "cpucore");
    if cores > 0.0 {
        let expect = number(&snap, "load", "min1") / cores * 100.0;
        assert!((number(&snap, "quicklook", "load") - expect).abs() < 1e-6);
    }
    if let Some(Value::String(n)) = obj.get("cpu_name") {
        assert!(!n.is_empty());
    }
}

#[test]
fn ticks_record_numeric_history_unless_disabled() {
    use std::sync::atomic::Ordering;
    let stats = live();
    assert!(stats.history_enabled.load(Ordering::Relaxed));
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mem = guard.iter().find(|p| p.name() == "mem").expect("mem plugin");
    let recorded = mem.model().expect("mem model").stats_history.get("percent", 0);
    assert!(!recorded.is_empty());
}

#[test]
fn disabled_history_records_nothing() {
    use std::sync::atomic::Ordering;
    let stats = GlancesStats::new(1.0);
    stats.history_enabled.store(false, Ordering::Relaxed);
    plugins::register_all(&stats);
    stats.update().unwrap();
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    for p in guard.iter() {
        if let Some(m) = p.model() {
            assert!(m.stats_history.snapshot().is_empty(), "{} recorded", p.name());
        }
    }
}

#[test]
fn list_plugins_record_element_series_with_cap() {
    use std::collections::BTreeMap;
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_all(&stats);
    let mut nic = BTreeMap::new();
    nic.insert("interface_name".into(), Value::String("eth0".into()));
    nic.insert("bytes_recv_rate_per_sec".into(), Value::Float(7.0));
    let mut guard = stats.plugins.write().unwrap();
    let p = guard.iter_mut().find(|p| p.name() == "network").expect("network");
    let m = p.model_mut().expect("model");
    m.stats = Value::Array(vec![Value::Object(nic)]);
    m.update_stats_history(&["bytes_recv_rate_per_sec"], Some("interface_name"), 3);
    let vals: Vec<f64> =
        m.stats_history.get("eth0_bytes_recv_rate_per_sec", 0).iter().map(|s| s.value).collect();
    assert_eq!(vals, vec![7.0]);
    for i in 0..5 {
        m.stats_history.add("eth0_bytes_recv_rate_per_sec", i as f64);
    }
    assert!(m.stats_history.get("eth0_bytes_recv_rate_per_sec", 0).len() <= 3);
}
