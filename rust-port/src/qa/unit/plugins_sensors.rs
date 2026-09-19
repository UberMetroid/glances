//! Tests for the sensors plugin — hwmon Value formatting and edge cases.

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::platform::linux::sys_class_hwmon::{self, HwmonSensor, SensorKind};
use crate::plugins::sensors::{sensor_to_value, NAME};

#[test]
fn name_and_register() {
    let s = GlancesStats::new(1.0);
    crate::plugins::sensors::register(&s);
    assert!(s.plugin_names().contains(&NAME));
}

#[test]
fn get_key_returns_label() {
    let p = crate::plugins::sensors::SensorsPlugin::new();
    assert_eq!(p.get_key(), Some("label"));
}

#[test]
fn sensor_to_value_temperature_uses_celsius_kind() {
    let s = HwmonSensor {
        chip: "k10temp".into(),
        label: "Tctl".into(),
        value: 65.0,
        kind: SensorKind::Temperature,
    };
    let v = sensor_to_value(&s);
    let obj = v.as_object().expect("object");
    assert_eq!(obj.get("label").and_then(Value::as_str), Some("Tctl"));
    assert_eq!(obj.get("chip").and_then(Value::as_str), Some("k10temp"));
    assert_eq!(obj.get("value").and_then(Value::as_f64), Some(65.0));
    assert_eq!(obj.get("kind").and_then(Value::as_str), Some("temperature_c"));
}

#[test]
fn sensor_to_value_fan_and_voltage_use_distinct_kinds() {
    let fan = HwmonSensor { chip: "nct6798".into(), label: "Fan1".into(), value: 1500.0, kind: SensorKind::Fan };
    let volt = HwmonSensor { chip: "nct6798".into(), label: "Vcore".into(), value: 1.2, kind: SensorKind::Voltage };
    assert_eq!(sensor_to_value(&fan).as_object().unwrap().get("kind").and_then(Value::as_str), Some("fan_rpm"));
    assert_eq!(sensor_to_value(&volt).as_object().unwrap().get("kind").and_then(Value::as_str), Some("voltage_v"));
}

#[test]
fn sensor_to_value_with_negative_temperature_succeeds() {
    // Edge case: subzero values are valid (freezer-room monitoring).
    let s = HwmonSensor { chip: "w83627".into(), label: "MB".into(), value: -10.5, kind: SensorKind::Temperature };
    let v = sensor_to_value(&s);
    assert_eq!(v.as_object().unwrap().get("value").and_then(Value::as_f64), Some(-10.5));
}

#[test]
fn plugin_update_returns_array_on_host_without_hwmon() {
    // /sys/class/hwmon may not exist on minimal containers; the plugin
    // must still emit a stable JSON shape (empty array).
    let mut p = crate::plugins::sensors::SensorsPlugin::new();
    p.update().expect("update must succeed even with no hwmon");
    assert!(p.stats().as_array().is_some(), "stats must always be an array");
}

#[test]
fn platform_read_all_is_ok_on_any_host() {
    // Either returns the sensors or an empty vec — never an error.
    assert!(sys_class_hwmon::read_all().is_ok());
}
