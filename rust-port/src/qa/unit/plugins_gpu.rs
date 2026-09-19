//! Tests for the GPU plugin — vendor detection + Value shape.

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins::gpu::{gpu_to_value, vendor_from_driver, GpuInfo, NAME};

#[test]
fn name_and_register() {
    let s = GlancesStats::new(1.0);
    crate::plugins::gpu::register(&s);
    assert!(s.plugin_names().contains(&NAME));
}

#[test]
fn get_key_returns_gpu_id() {
    let p = crate::plugins::gpu::GpuPlugin::new();
    assert_eq!(p.get_key(), Some("gpu_id"));
}

#[test]
fn vendor_from_driver_maps_known_drivers() {
    assert_eq!(vendor_from_driver("amdgpu"), "amd");
    assert_eq!(vendor_from_driver("i915"), "intel");
    assert_eq!(vendor_from_driver("nouveau"), "nvidia");
    assert_eq!(vendor_from_driver("tegra-drm"), "tegra");
    assert_eq!(vendor_from_driver("radeon"), "amd");
    // Unknown drivers are preserved verbatim.
    assert_eq!(vendor_from_driver("vmwgfx"), "vmware");
    assert_eq!(vendor_from_driver("my_custom_driver"), "my_custom_driver");
}

#[test]
fn gpu_to_value_emits_canonical_keys() {
    let g = GpuInfo {
        gpu_id: "0000:03:00.0".into(),
        vendor: "amd".into(),
        name: "Radeon RX 7900 XT".into(),
        util_pct: Some(42.0),
        freq_mhz: Some(2400.0),
    };
    let v = gpu_to_value(&g);
    let obj = v.as_object().expect("object");
    assert_eq!(obj.get("gpu_id").and_then(Value::as_str), Some("0000:03:00.0"));
    assert_eq!(obj.get("vendor").and_then(Value::as_str), Some("amd"));
    assert_eq!(obj.get("name").and_then(Value::as_str), Some("Radeon RX 7900 XT"));
    assert_eq!(obj.get("util_pct").and_then(Value::as_f64), Some(42.0));
    assert_eq!(obj.get("freq_mhz").and_then(Value::as_f64), Some(2400.0));
}

#[test]
fn gpu_to_value_handles_missing_optional_fields() {
    let g = GpuInfo {
        gpu_id: "0000:00:02.0".into(),
        vendor: "intel".into(),
        name: "Meteor Lake".into(),
        util_pct: None,
        freq_mhz: None,
    };
    let v = gpu_to_value(&g);
    let obj = v.as_object().unwrap();
    // Missing fields must serialize as JSON null (Value::Null), not 0.
    assert!(matches!(obj.get("util_pct"), Some(Value::Null)));
    assert!(matches!(obj.get("freq_mhz"), Some(Value::Null)));
}

#[test]
fn plugin_update_emits_array_even_without_gpus() {
    let mut p = crate::plugins::gpu::GpuPlugin::new();
    p.update().expect("update must not error");
    assert!(p.stats().as_array().is_some(), "stats always an array");
}

#[test]
fn plugin_reset_clears_stats_to_empty_array() {
    let mut p = crate::plugins::gpu::GpuPlugin::new();
    p.update().expect("update ok");
    p.reset();
    assert!(p.stats().as_array().unwrap().is_empty());
}
