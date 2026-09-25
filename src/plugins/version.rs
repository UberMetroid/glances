//! Version plugin — build and protocol metadata, all static.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "version";

/// This build's version, straight from the package manifest.
pub const GLANCES_VERSION: &str = env!("CARGO_PKG_VERSION");

/// REST/MCP protocol version. Bumped on breaking changes; consumers
/// feature-detect against this string.
pub const API_VERSION: &str = "4";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(VersionPlugin::new()));
}

/// The static payload. Public so tests assert against the same source
/// of truth the plugin serves.
pub fn stats_payload() -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("glances_version".to_string(), Value::String(GLANCES_VERSION.to_string())),
        ("api_version".to_string(), Value::String(API_VERSION.to_string())),
        ("plugin_version".to_string(), Value::String(API_VERSION.to_string())),
        ("crustacean".to_string(), Value::String("rust".to_string())),
        ("std".to_string(), Value::String("true".to_string())),
    ])
}

pub struct VersionPlugin { base: GlancesPluginModel }

impl Default for VersionPlugin {
    fn default() -> Self { Self::new() }
}

impl VersionPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Object(stats_payload())) }
    }
}

impl Plugin for VersionPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_identity() {
        assert_eq!(VersionPlugin::new().name(), "version");
    }
    #[test]
    fn payload_keys_are_all_strings() {
        let obj = stats_payload();
        for k in ["glances_version", "api_version", "plugin_version", "crustacean", "std"] {
            assert!(matches!(obj.get(k), Some(Value::String(_))), "{k} must be a string");
        }
    }
    #[test]
    fn payload_values() {
        let obj = stats_payload();
        assert!(!GLANCES_VERSION.is_empty() && !API_VERSION.is_empty());
        assert_eq!(obj["glances_version"].as_str(), Some(GLANCES_VERSION));
        assert_eq!(obj["api_version"].as_str(), Some("4"));
        assert_eq!(obj["plugin_version"].as_str(), Some("4"));
        assert_eq!(obj["crustacean"].as_str(), Some("rust"));
        assert_eq!(obj["std"].as_str(), Some("true"));
    }
    #[test]
    fn reset_and_update_keep_statics() {
        let mut p = VersionPlugin::new();
        p.stats_mut().as_object_mut().unwrap().insert("std".into(), Value::String("x".into()));
        p.reset();
        assert_eq!(p.stats().as_object().unwrap()["std"].as_str(), Some("true"));
        let before = p.stats().clone();
        p.update().unwrap();
        assert_eq!(p.stats(), &before);
    }
}
