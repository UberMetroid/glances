//! Alert plugin — the shared event log as alert records.
//!
//! The threshold engine appends crossings to the global log; this
//! plugin surfaces them newest-first (capped at 100) with type word,
//! stat, value, and epoch timestamp. `update` is a no-op — views
//! rebuild from the log after every tick.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::threshold::Severity;
use crate::core::value::Value;

pub const NAME: &str = "alert";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(AlertPlugin::new()));
}

pub struct AlertPlugin { base: GlancesPluginModel }

impl Default for AlertPlugin {
    fn default() -> Self { Self::new() }
}

impl AlertPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }

    /// Alerts currently held in stats.
    pub fn count(&self) -> usize {
        self.base.stats.as_array().map(|a| a.len()).unwrap_or(0)
    }
}

impl Plugin for AlertPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> { Ok(()) }
    fn update_views(&mut self, events: &mut EventLog) {
        let arr: Vec<Value> = events.snapshot().iter().rev().take(100).map(|e| {
            let mut o = BTreeMap::new();
            o.insert("type".into(), Value::String(severity_word(e.severity).into()));
            o.insert("stat".into(), Value::String(e.stat.clone()));
            o.insert("value".into(), Value::Float(e.value));
            o.insert("timestamp".into(), Value::Float(epoch_secs(e.timestamp)));
            Value::Object(o)
        }).collect();
        self.base.stats = Value::Array(arr);
    }
}

fn severity_word(s: Severity) -> &'static str {
    match s {
        Severity::Ok => "OK",
        Severity::Careful => "CAREFUL",
        Severity::Warning => "WARNING",
        Severity::Critical => "CRITICAL",
    }
}

fn epoch_secs(t: std::time::SystemTime) -> f64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_identity() {
        assert_eq!(AlertPlugin::new().name(), "alert");
    }
    #[test]
    fn fresh_state_holds_nothing() {
        let p = AlertPlugin::new();
        assert_eq!(p.count(), 0);
        assert!(p.stats().as_array().is_some_and(|a| a.is_empty()));
    }
    #[test]
    fn update_keeps_state_and_reset_clears() {
        let mut p = AlertPlugin::new();
        p.update().unwrap();
        assert_eq!(p.count(), 0);
        p.stats_mut().as_array_mut().unwrap().push(Value::Object(Default::default()));
        assert_eq!(p.count(), 1);
        p.reset();
        assert_eq!(p.count(), 0);
    }
    #[test]
    fn views_surface_the_log_newest_first() {
        use crate::core::events::Event;
        let mut log = EventLog::default();
        for (sev, stat) in [(Severity::Ok, "a"), (Severity::Critical, "b")] {
            log.push(Event { severity: sev, stat: stat.into(), value: 1.0, timestamp: std::time::SystemTime::now() });
        }
        let mut p = AlertPlugin::new();
        p.update_views(&mut log);
        let arr = p.stats().as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_object().unwrap()["type"].as_str(), Some("CRITICAL"));
        assert_eq!(arr[1].as_object().unwrap()["type"].as_str(), Some("OK"));
    }
}
