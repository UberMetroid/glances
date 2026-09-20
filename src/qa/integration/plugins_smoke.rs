//! End-to-end smoke test for the core plugins (cpu, mem, memswap, load,
//! uptime, now, system). On a Linux host, each plugin should update() OK
//! and produce a non-empty stats object with at least one numeric field.

use crate::core::error::Result;
use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::plugins::{cpu, load, mem, memswap, now, system, uptime};

fn assert_has_numeric_field(stats: &crate::core::value::Value, name: &str) {
    let obj = stats.as_object().expect("stats must be an object");
    let v = obj.get(name).expect(&format!("missing field '{}'", name));
    assert!(v.as_f64().is_some(), "field '{}' should be numeric", name);
}

#[test]
fn mem_plugin_reads_real_system() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = mem::MemPlugin::new();
    p.update().expect("mem update should succeed on Linux");
    let s = p.stats();
    assert_has_numeric_field(s, "total");
    assert_has_numeric_field(s, "used");
    assert_has_numeric_field(s, "free");
    assert_has_numeric_field(s, "percent");
    let total = s.as_object().unwrap().get("total").unwrap().as_f64().unwrap();
    assert!(total > 0.0, "total RAM should be > 0; got {}", total);
}

#[test]
fn memswap_plugin_reads_real_system() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = memswap::MemswapPlugin::new();
    p.update().expect("memswap update should succeed");
    assert_has_numeric_field(p.stats(), "total");
}

#[test]
fn load_plugin_reads_real_system() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = load::LoadPlugin::new();
    p.update().expect("load update should succeed");
    assert_has_numeric_field(p.stats(), "min1");
    assert_has_numeric_field(p.stats(), "min5");
    assert_has_numeric_field(p.stats(), "min15");
}

#[test]
fn uptime_plugin_reads_real_system() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = uptime::UptimePlugin::new();
    p.update().expect("uptime update should succeed");
    let s = p.stats().as_object().unwrap();
    let secs = s.get("seconds").unwrap().as_f64().unwrap();
    assert!(secs > 0.0, "uptime should be > 0");
}

#[test]
fn now_plugin_produces_iso_string() {
    let mut p = now::NowPlugin::new();
    p.update().expect("now update should succeed");
    let s = p.stats().as_object().unwrap();
    let iso = s.get("iso").unwrap().as_str().unwrap();
    assert!(!iso.is_empty(), "iso string should not be empty");
    assert!(iso.contains('T'), "iso should contain 'T' separator");
}

#[test]
fn system_plugin_reads_hostname() {
    let mut p = system::SystemPlugin::new();
    p.update().expect("system update should succeed");
    let s = p.stats().as_object().unwrap();
    let h = s.get("hostname").unwrap().as_str().unwrap();
    assert!(!h.is_empty(), "hostname should not be empty");
    let arch = s.get("arch").unwrap().as_str().unwrap();
    assert!(!arch.is_empty(), "arch should not be empty");
}

#[test]
fn cpu_plugin_two_tick_produces_percentages() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = cpu::CpuPlugin::new();
    p.update().expect("first cpu update should succeed");
    p.update().expect("second cpu update should succeed");
    let s = p.stats().as_object().unwrap();
    let total = s.get("total").and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert!(total >= 0.0 && total <= 100.0, "cpu.total out of range: {}", total);
}

#[test]
fn register_all_then_update() -> Result<()> {
    if !cfg!(target_os = "linux") { return Ok(()); }
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_all(&stats);
    stats.update()?;
    let names = stats.plugin_names();
    assert_eq!(names, vec!["cpu", "percpu", "processcount", "ip", "mem", "memswap", "load", "uptime", "now", "system", "fs", "diskio", "folders", "raid", "network", "connections", "ports", "containers", "cloud", "amps", "sensors", "gpu", "npu", "wifi", "mpp", "alert", "quicklook", "help", "version", "psutilversion"]);
    Ok(())
}
