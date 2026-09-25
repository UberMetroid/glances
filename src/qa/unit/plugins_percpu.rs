//! Tests for the per-CPU plugin.

use crate::core::plugin::Plugin;
use crate::core::value::Value;
use crate::platform::linux::proc_stat;
use crate::plugins::percpu;

const NORMAL: &str = include_str!("../fixtures/proc/stat_normal.txt");

#[test]
fn plugin_metadata_matches_other_plugins() {
    let p = percpu::PerCpuPlugin::new();
    assert_eq!(p.name(), percpu::NAME);
    assert_eq!(p.name(), "percpu");
    assert_eq!(p.get_key(), Some("cpu_number"));
}

#[test]
fn update_produces_one_entry_per_cpu() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = percpu::PerCpuPlugin::new();
    p.update().expect("percpu update should succeed on Linux");
    let arr = p.stats().as_array().expect("stats must be an array");
    let expected_cpus = proc_stat::parse(NORMAL).unwrap().per_cpu.len();
    if expected_cpus > 0 {
        assert!(arr.len() >= expected_cpus,
            "expected at least {} entries, got {}", expected_cpus, arr.len());
    }
    // Every entry must be an Object with a cpu_number key.
    for entry in arr {
        let obj = entry.as_object().expect("entry must be an object");
        let key = obj.get("cpu_number").expect("missing cpu_number key");
        assert!(matches!(key, Value::String(_)), "cpu_number should be a string");
        assert_eq!(obj.get("key").and_then(Value::as_str), Some("cpu_number"));
        assert!(obj.contains_key("user"));
        assert!(obj.contains_key("system"));
        assert!(obj.contains_key("idle"));
        assert!(obj.contains_key("total"));
        assert!(obj.contains_key("busy"));
    }
}

#[test]
fn cpu_number_keys_are_unique() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = percpu::PerCpuPlugin::new();
    p.update().expect("percpu update should succeed");
    let arr = p.stats().as_array().unwrap();
    let mut seen = std::collections::HashSet::new();
    for entry in arr {
        let key = entry.as_object().unwrap().get("cpu_number").unwrap()
            .as_str().unwrap().to_string();
        assert!(seen.insert(key.clone()), "duplicate cpu_number: {}", key);
    }
}

#[test]
fn reset_restores_empty_array() {
    let mut p = percpu::PerCpuPlugin::new();
    // Force a non-empty state by replacing stats directly (via stats_mut).
    *p.stats_mut() = Value::Array(vec![Value::Object(Default::default())]);
    assert!(p.stats().as_array().unwrap().len() == 1);
    p.reset();
    // After reset we should be back to the initial empty array.
    let arr = p.stats().as_array().expect("stats must be an array");
    assert!(arr.is_empty(), "expected empty array after reset, got {} entries", arr.len());
}

#[test]
fn first_tick_reports_zero_percentages() {
    if !cfg!(target_os = "linux") { return; }
    // No previous sample → percentages must be 0.0, not raw jiffies.
    let mut p = percpu::PerCpuPlugin::new();
    p.update().expect("update");
    let arr = p.stats().as_array().unwrap();
    if arr.is_empty() { return; }
    let obj0 = arr[0].as_object().unwrap();
    for k in ["busy", "total", "user", "system", "idle"] {
        let v = obj0.get(k).and_then(|v| v.as_f64()).unwrap_or(-1.0);
        assert_eq!(v, 0.0, "first tick must emit 0.0 for {}, not jiffies", k);
    }
}

#[test]
fn second_tick_reports_bounded_percentages() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = percpu::PerCpuPlugin::new();
    p.update().expect("update 1");
    p.update().expect("update 2");
    let arr = p.stats().as_array().unwrap();
    for (i, entry) in arr.iter().enumerate() {
        let obj = entry.as_object().unwrap();
        for k in ["busy", "total", "user", "system", "idle", "iowait"] {
            let v = obj.get(k).and_then(|v| v.as_f64()).unwrap_or(-1.0);
            assert!((0.0..=100.0).contains(&v),
                "cpu{} {} must be a percentage, got {}", i, k, v);
        }
    }
}