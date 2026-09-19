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
fn busy_and_total_match_proc_stat() {
    if !cfg!(target_os = "linux") { return; }
    // Re-read /proc/stat and verify the plugin's busy/total match
    // (rather than comparing against the static fixture, which has
    // different absolute values from the live system).
    let live = proc_stat::read().expect("read live /proc/stat");
    if live.per_cpu.is_empty() { return; }
    let t = &live.per_cpu[0];
    let mut p = percpu::PerCpuPlugin::new();
    p.update().expect("update");
    let arr = p.stats().as_array().unwrap();
    let obj0 = arr[0].as_object().unwrap();
    let busy = obj0.get("busy").and_then(|v| v.as_f64()).unwrap_or(-1.0);
    let total = obj0.get("total").and_then(|v| v.as_f64()).unwrap_or(-1.0);
    assert_eq!(busy, t.busy() as f64, "busy mismatch on live /proc/stat");
    assert_eq!(total, t.total() as f64, "total mismatch on live /proc/stat");
}