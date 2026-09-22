//! Pressure-stall plugin — CPU/memory/IO "some" avg10 from PSI.
//!
//! Linux-only (`/proc/pressure/*`, kernel 4.20+). Each value is the
//! percent of wall time some task stalled on that resource over the
//! last 10s. Missing files (older kernels, non-Linux) read Null.

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "pressure";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(PressurePlugin::new()));
}

/// Parse the `some` line's avg10 from PSI text. Missing → None.
pub fn parse_some_avg10(text: &str) -> Option<f64> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("some ") {
            for field in rest.split_whitespace() {
                if let Some(v) = field.strip_prefix("avg10=") {
                    return v.parse::<f64>().ok();
                }
            }
        }
    }
    None
}

fn read_avg10(resource: &str) -> Value {
    match fs::read_to_string(format!("/proc/pressure/{}", resource)) {
        Ok(t) => parse_some_avg10(&t).map(Value::Float).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

pub struct PressurePlugin {
    base: GlancesPluginModel,
}

impl PressurePlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        for k in &["cpu", "mem", "io"] {
            m.insert(k.to_string(), Value::Null);
        }
        Self {
            base: GlancesPluginModel::new(NAME, Value::Object(m)),
        }
    }
}

impl Plugin for PressurePlugin {
    fn name(&self) -> &'static str {
        NAME
    }
    fn reset(&mut self) {
        self.base.reset();
    }
    fn stats(&self) -> &Value {
        &self.base.stats
    }
    fn model(&self) -> Option<&GlancesPluginModel> {
        Some(&self.base)
    }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> {
        Some(&mut self.base)
    }
    fn stats_mut(&mut self) -> &mut Value {
        &mut self.base.stats
    }
    fn update(&mut self) -> Result<()> {
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("cpu".into(), read_avg10("cpu"));
            obj.insert("mem".into(), read_avg10("memory"));
            obj.insert("io".into(), read_avg10("io"));
        }
        Ok(())
    }
    fn update_views(&mut self, _events: &mut EventLog) {
        // No thresholds defined: views stay empty.
        if let Some(m) = self.model_mut() {
            m.views = std::collections::HashMap::new();
        }
    }
}
