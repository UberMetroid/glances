//! Quicklook plugin — the top status bar's compact summary.
//!
//! Holds cpu/mem/swap/load/cpu_name. Values arrive from the stats
//! post-pass aggregation (which reads the sibling plugins after each
//! tick) — this plugin has no data source of its own.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "quicklook";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(QuicklookPlugin::new()));
}

/// One summary row: `{key, label, value, unit}`.
pub type QuicklookRow = BTreeMap<String, Value>;

pub struct QuicklookPlugin { base: GlancesPluginModel }

impl Default for QuicklookPlugin {
    fn default() -> Self { Self::new() }
}

impl QuicklookPlugin {
    pub fn new() -> Self {
        let m = BTreeMap::from([
            ("cpu".to_string(), Value::Float(0.0)),
            ("mem".to_string(), Value::Float(0.0)),
            ("swap".to_string(), Value::Float(0.0)),
            ("load".to_string(), Value::Float(0.0)),
            ("cpu_name".to_string(), Value::String(String::new())),
        ]);
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

impl Plugin for QuicklookPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["cpu", "percpu", "mem", "swap", "load"] }
    fn update(&mut self) -> Result<()> { Ok(()) }
    fn update_views(&mut self, events: &mut EventLog) {
        let Some(m) = self.model_mut() else { return };
        m.build_views(&[], None, None);
        let readings: Vec<(String, f64)> = match m.stats.as_object() {
            Some(o) => ["cpu", "mem", "swap"]
                .iter()
                .filter_map(|k| o.get(*k).and_then(Value::as_f64).map(|v| (k.to_string(), v)))
                .collect(),
            None => return,
        };
        for (k, v) in readings {
            let d = m.get_alert(v, 0.0, 100.0, &k, None, false, false, None, Some(&mut *events));
            m.views.entry(String::new()).or_default().insert(k, d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_identity() {
        assert_eq!(QuicklookPlugin::new().name(), "quicklook");
    }
    #[test]
    fn fresh_state_has_five_keys() {
        let obj = QuicklookPlugin::new().stats().clone();
        let obj = obj.as_object().unwrap();
        assert_eq!(obj.len(), 5);
        for k in ["cpu", "mem", "swap", "load", "cpu_name"] {
            assert!(obj.contains_key(k), "missing {k}");
        }
    }
    #[test]
    fn standalone_tick_keeps_zeros() {
        // Without a stats tick there is no aggregation source.
        let mut p = QuicklookPlugin::new();
        p.update().unwrap();
        let obj = p.stats().as_object().unwrap();
        assert_eq!(obj["cpu"].as_f64(), Some(0.0));
        assert_eq!(obj["mem"].as_f64(), Some(0.0));
    }
    #[test]
    fn reset_restores_defaults() {
        let mut p = QuicklookPlugin::new();
        p.stats_mut().as_object_mut().unwrap().insert("cpu".into(), Value::Float(87.5));
        p.reset();
        assert_eq!(p.stats().as_object().unwrap()["cpu"].as_f64(), Some(0.0));
    }
}
