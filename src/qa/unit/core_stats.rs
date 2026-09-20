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
