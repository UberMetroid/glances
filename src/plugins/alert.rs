//! Alert plugin — list of currently-triggered threshold alerts.
//!
//! Mirrors `glances/plugins/alert/__init__.py`. The actual threshold
//! evaluation lives in `crate::core::threshold`; this plugin is the
//! display-side surface that the UI consumes.
//!
//! M11 ships a display-only stub: the stats value is always an empty
//! array. As soon as the threshold engine starts emitting alert records
//! (M11-followup), `update()` will populate it.

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

impl AlertPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }

    /// Number of alerts currently held in `stats`.
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
    fn update(&mut self) -> Result<()> {
        // Stats are rebuilt from the shared event log in update_views
        // (the refresh loop always runs views after update).
        Ok(())
    }
    fn update_views(&mut self, events: &mut EventLog) {
        // Surface the global event log as alert records (upstream alert
        // plugin parity: one entry per active threshold breach).
        let mut arr = Vec::new();
        for e in events.snapshot().iter().rev().take(100) {
            let mut o = BTreeMap::new();
            o.insert("type".into(), Value::String(severity_word(e.severity).into()));
            o.insert("stat".into(), Value::String(e.stat.clone()));
            o.insert("value".into(), Value::Float(e.value));
            let ts = e.timestamp
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            o.insert("timestamp".into(), Value::Float(ts));
            arr.push(Value::Object(o));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_alert() {
        let p = AlertPlugin::new();
        assert_eq!(p.name(), NAME);
        assert_eq!(p.name(), "alert");
    }

    #[test]
    fn new_starts_with_empty_array() {
        let p = AlertPlugin::new();
        let arr = p.stats().as_array().expect("stats must be an array");
        assert!(arr.is_empty(), "fresh AlertPlugin must have zero entries");
        assert_eq!(p.count(), 0);
    }

    #[test]
    fn update_leaves_stats_empty() {
        let mut p = AlertPlugin::new();
        p.update().expect("update should not fail");
        let arr = p.stats().as_array().expect("stats must remain an array");
        assert!(arr.is_empty(), "M11 stub update must not push alerts");
        assert_eq!(p.count(), 0);
    }

    #[test]
    fn reset_clears_external_mutations() {
        let mut p = AlertPlugin::new();
        // Simulate someone pushing an alert record directly.
        p.stats_mut().as_array_mut().unwrap().push(Value::Object(Default::default()));
        assert_eq!(p.count(), 1);
        p.reset();
        assert_eq!(p.count(), 0, "reset must restore the initial empty array");
    }
}