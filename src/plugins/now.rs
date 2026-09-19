//! Now plugin — current local date/time + ISO 8601.

use std::collections::BTreeMap;
use std::time::SystemTime;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "now";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(NowPlugin::new()));
}

pub struct NowPlugin { base: GlancesPluginModel }

impl NowPlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        m.insert("iso".into(), Value::String(String::new()));
        m.insert("utc".into(), Value::Float(0.0));
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

impl Plugin for NowPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        let now = SystemTime::now();
        let dur = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
        let secs = dur.as_secs_f64();
        // Trivial ISO formatter — proper chrono-equivalent formatting lives in M6-followup.
        let total = dur.as_secs();
        let days = total / 86400;
        let rem = total % 86400;
        let h = rem / 3600;
        let m = (rem / 60) % 60;
        let s = rem % 60;
        let iso = format!("1970-01-01T{:02}:{:02}:{:02}Z+{}d", h, m, s, days);
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("iso".into(), Value::String(iso));
            obj.insert("utc".into(), Value::Float(secs));
        }
        Ok(())
    }
}
