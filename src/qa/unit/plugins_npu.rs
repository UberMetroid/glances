//! Tests for the NPU plugin — discovery + Value shape.

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins::npu::{npu_to_value, scan_bus_root, vendor_from_driver, NpuInfo, NAME};
use crate::qa::harness::TempDir;

#[test]
fn name_and_register() {
    let s = GlancesStats::new(1.0);
    crate::plugins::npu::register(&s);
    assert!(s.plugin_names().contains(&NAME));
}

#[test]
fn get_key_returns_npu_id() {
    let p = crate::plugins::npu::NpuPlugin::new();
    assert_eq!(p.get_key(), Some("npu_id"));
}

#[test]
fn vendor_from_driver_recognises_xdna_and_vpu() {
    assert_eq!(vendor_from_driver("amdxdna"), "amd");
    assert_eq!(vendor_from_driver("intel_vpu"), "intel");
    assert_eq!(vendor_from_driver("ivpu"), "intel");
    assert_eq!(vendor_from_driver("accel"), "intel");
    assert_eq!(vendor_from_driver("vaim"), "intel");
    // Unknown drivers return "unknown" — never panic, never a fake vendor.
    assert_eq!(vendor_from_driver("foo"), "unknown");
    assert_eq!(vendor_from_driver("nvidia"), "unknown");
}

#[test]
fn npu_to_value_emits_canonical_keys() {
    let n = NpuInfo {
        npu_id: "npu0".into(),
        path: "/sys/devices/pci0000:00/0000:00:08.1/npu".into(),
        vendor: "amd".into(),
        util_pct: Some(75.5),
        freq_mhz: Some(1500.0),
    };
    let v = npu_to_value(&n);
    let obj = v.as_object().expect("object");
    assert_eq!(obj.get("npu_id").and_then(Value::as_str), Some("npu0"));
    assert_eq!(obj.get("vendor").and_then(Value::as_str), Some("amd"));
    assert_eq!(obj.get("util_pct").and_then(Value::as_f64), Some(75.5));
    assert_eq!(obj.get("freq_mhz").and_then(Value::as_f64), Some(1500.0));
}

#[test]
fn npu_to_value_missing_fields_become_null() {
    let n = NpuInfo {
        npu_id: "vpu0".into(),
        path: "/sys/devices/.../vpu".into(),
        vendor: "intel".into(),
        util_pct: None,
        freq_mhz: None,
    };
    let v = npu_to_value(&n);
    let obj = v.as_object().unwrap();
    assert!(matches!(obj.get("util_pct"), Some(Value::Null)));
    assert!(matches!(obj.get("freq_mhz"), Some(Value::Null)));
}

#[test]
fn plugin_update_returns_empty_array_on_host_without_npu() {
    // Most test hosts have no NPU — the plugin must emit [] without
    // raising an error so the JSON shape stays stable.
    let mut p = crate::plugins::npu::NpuPlugin::new();
    p.update().expect("update must not error");
    let arr = p.stats().as_array().expect("stats must be array");
    assert!(arr.len() <= 8, "host likely has no NPU; large array is suspect");
    for entry in arr {
        let obj = entry.as_object().expect("entry must be object");
        assert!(obj.contains_key("npu_id"));
        assert!(obj.contains_key("vendor"));
    }
}

#[test]
fn plugin_reset_clears_stats() {
    let mut p = crate::plugins::npu::NpuPlugin::new();
    p.update().expect("update ok");
    p.reset();
    assert!(p.stats().as_array().unwrap().is_empty());
}

#[test]
fn scan_bus_root_matches_name_prefix_and_driver_only() {
    // Regression: discovery must be a flat bus scan, never a deep
    // /sys/devices walk (~15s/tick). Fake bus root, no hardware.
    let dir = TempDir::new("npu-bus-scan");
    let root = dir.path();
    for d in ["npu0", "input0", "0000:01:00.0", "0000:02:00.0"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::os::unix::fs::symlink("../../../../bus/pci/drivers/amdxdna",
        root.join("0000:01:00.0/driver")).unwrap();
    std::os::unix::fs::symlink("../../../../bus/pci/drivers/ahci",
        root.join("0000:02:00.0/driver")).unwrap();
    let mut out = Vec::new();
    scan_bus_root(root, &mut out);
    let mut names: Vec<String> = out.iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["0000:01:00.0".to_string(), "npu0".to_string()]);
}
