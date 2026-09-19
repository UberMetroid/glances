//! Tests for Linux /sys/class/hwmon reader.

use crate::platform::linux::sys_class_hwmon::{self, SensorKind};

#[test]
fn read_all_returns_or_errors_gracefully() {
    // /sys/class/hwmon may not exist on minimal containers.
    let r = sys_class_hwmon::read_all();
    assert!(r.is_ok(), "hwmon read should at least return Ok with empty vec");
}

#[test]
fn sensor_kind_suffix() {
    assert_eq!(SensorKind::Temperature.suffix(), "input");
    assert_eq!(SensorKind::Fan.suffix(), "input");
    assert_eq!(SensorKind::Voltage.suffix(), "input");
}
