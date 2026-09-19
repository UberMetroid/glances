//! Sensors plugin — temperature, fan, voltage from /sys/class/hwmon.
//!
//! Wraps `platform::linux::sys_class_hwmon::read_all()`. Each sensor becomes
//! one dict entry; the dict key used for identification is `label` (the
//! per-sensor label exposed by sysfs, e.g. "CPU Temperature"). The `kind`
//! string is one of "temperature_c", "fan_rpm", "voltage_v".

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "sensors";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(SensorsPlugin::new()));
}

/// Convert one HwmonSensor record into the canonical Value shape.
/// Exposed for unit tests so the formatter can be exercised without
/// needing /sys/class/hwmon to exist on the test host.
pub fn sensor_to_value(s: &plat::linux::sys_class_hwmon::HwmonSensor) -> Value {
    let kind_str = match s.kind {
        plat::linux::sys_class_hwmon::SensorKind::Temperature => "temperature_c",
        plat::linux::sys_class_hwmon::SensorKind::Fan => "fan_rpm",
        plat::linux::sys_class_hwmon::SensorKind::Voltage => "voltage_v",
    };
    let mut obj = BTreeMap::new();
    obj.insert("label".into(), Value::String(s.label.clone()));
    obj.insert("chip".into(), Value::String(s.chip.clone()));
    obj.insert("value".into(), Value::Float(s.value));
    obj.insert("kind".into(), Value::String(kind_str.to_string()));
    Value::Object(obj)
}

pub struct SensorsPlugin { base: GlancesPluginModel }

impl SensorsPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

impl Default for SensorsPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for SensorsPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn get_key(&self) -> Option<&'static str> { Some("label") }

    fn update(&mut self) -> Result<()> {
        // read_all() never errors on missing /sys/class/hwmon — it
        // returns an empty vec instead, so the plugin shape is stable
        // on hosts with no sensors (containers, embedded boards).
        let sensors = plat::linux::sys_class_hwmon::read_all()?;
        let out: Vec<Value> = sensors.iter().map(sensor_to_value).collect();
        self.base.stats = Value::Array(out);
        Ok(())
    }
}
