//! Psutilversion plugin — placeholder for Python Glances' psutil version.
//!
//! The Python project reads `psutil.__version__` and exposes it as a
//! plugin so users can see which psutil release their stats came from.
//! The Rust port does not (and cannot, per AC-1) depend on psutil — it
//! reads `/proc`, `/sys`, and the `libc`/`Win32`/`Mach` APIs directly.
//!
//! M11 keeps the plugin registered so consumers wiring up dashboards
//! don't have to special-case the rust flavour; the `available` field
//! reports `false` and `version` carries a short explanation.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "psutilversion";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(PsutilversionPlugin::new()));
}

/// Static payload for this plugin.
pub fn stats_payload() -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    m.insert("available".into(), Value::Bool(false));
    m.insert("version".into(),   Value::String("n/a (rust port; no psutil dependency)".into()));
    m.insert("backend".into(),   Value::String("std + platform FFI".into()));
    m
}

pub struct PsutilversionPlugin { base: GlancesPluginModel }

impl PsutilversionPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Object(stats_payload())),
        }
    }
}

impl Plugin for PsutilversionPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        // Placeholder: values are static, no I/O to perform.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_psutilversion() {
        let p = PsutilversionPlugin::new();
        assert_eq!(p.name(), NAME);
        assert_eq!(p.name(), "psutilversion");
    }

    #[test]
    fn stats_payload_marks_unavailable() {
        let obj = stats_payload();
        assert_eq!(obj.get("available"), Some(&Value::Bool(false)),
            "available must be false on the rust port");
        assert!(obj.contains_key("version"));
        assert!(obj.contains_key("backend"));
        assert_eq!(obj.len(), 3);
    }

    #[test]
    fn version_string_mentions_rust() {
        let obj = stats_payload();
        let v = obj["version"].as_str().unwrap();
        assert!(v.to_lowercase().contains("rust"),
            "version string should mention rust, got: {v}");
        assert!(v.contains("n/a") || v.contains("not applicable") || v.contains("none"),
            "version string should mark psutil as not in use, got: {v}");
    }

    #[test]
    fn plugin_stats_match_payload() {
        let p = PsutilversionPlugin::new();
        let plugin_obj = p.stats().as_object().unwrap();
        let payload = stats_payload();
        assert_eq!(plugin_obj.len(), payload.len());
        for (k, v) in payload {
            assert_eq!(plugin_obj.get(&k), Some(&v), "mismatch on key {k}");
        }
    }

    #[test]
    fn reset_restores_placeholder() {
        let mut p = PsutilversionPlugin::new();
        p.stats_mut().as_object_mut().unwrap()
            .insert("available".into(), Value::Bool(true));
        p.reset();
        assert_eq!(
            p.stats().as_object().unwrap().get("available"),
            Some(&Value::Bool(false)),
            "reset must restore the original placeholder",
        );
    }
}