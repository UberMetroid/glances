//! Quicklook plugin — compact per-plugin summary used by the top status bar.
//!
//! Mirrors `glances/plugins/quicklook/__init__.py`. Each entry is a
//! short label + percent/value pair (CPU%, MEM%, LOAD, SWAP%).
//!
//! Values are filled by `GlancesStats::update`'s post-pass aggregation
//! (`aggregate_quicklook` in core/stats.rs) which reads the sibling
//! cpu/mem/memswap/load plugin stats after each tick.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "quicklook";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(QuicklookPlugin::new()));
}

/// One row of the quicklook summary: `{key, label, value, unit}`.
pub type QuicklookRow = BTreeMap<String, Value>;

pub struct QuicklookPlugin { base: GlancesPluginModel }

impl QuicklookPlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        m.insert("cpu".into(),    Value::Float(0.0));
        m.insert("mem".into(),    Value::Float(0.0));
        m.insert("swap".into(),   Value::Float(0.0));
        m.insert("load".into(),   Value::Float(0.0));
        m.insert("cpu_name".into(), Value::String(String::new()));
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
    fn update(&mut self) -> Result<()> {
        // Cross-plugin values are filled by the stats post-pass
        // (aggregate_quicklook); the plugin itself has no /proc source.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_quicklook() {
        let p = QuicklookPlugin::new();
        assert_eq!(p.name(), NAME);
        assert_eq!(p.name(), "quicklook");
    }

    #[test]
    fn new_has_all_summary_keys() {
        let p = QuicklookPlugin::new();
        let obj = p.stats().as_object().expect("stats must be an object");
        for k in ["cpu", "mem", "swap", "load", "cpu_name"] {
            assert!(obj.contains_key(k), "missing summary key: {k}");
        }
        assert_eq!(obj.len(), 5);
    }

    #[test]
    fn percentage_fields_default_to_zero() {
        let p = QuicklookPlugin::new();
        let obj = p.stats().as_object().unwrap();
        for k in ["cpu", "mem", "swap", "load"] {
            assert_eq!(
                obj.get(k).and_then(Value::as_f64),
                Some(0.0),
                "{k} must start at 0.0%",
            );
        }
    }

    #[test]
    fn update_is_idempotent_standalone() {
        // Outside a GlancesStats tick there is no aggregation source, so
        // the plugin keeps its defaults.
        let mut p = QuicklookPlugin::new();
        p.update().expect("update should not fail");
        let obj = p.stats().as_object().unwrap();
        assert_eq!(obj.get("cpu").and_then(Value::as_f64), Some(0.0));
        assert_eq!(obj.get("mem").and_then(Value::as_f64), Some(0.0));
    }

    #[test]
    fn reset_restores_initial_state() {
        let mut p = QuicklookPlugin::new();
        // Stomp a value via stats_mut to verify reset semantics.
        p.stats_mut().as_object_mut().unwrap()
            .insert("cpu".into(), Value::Float(87.5));
        p.reset();
        assert_eq!(
            p.stats().as_object().unwrap().get("cpu").and_then(Value::as_f64),
            Some(0.0),
        );
    }
}