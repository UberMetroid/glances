//! Version plugin — exposes build + runtime version metadata.
//!
//! Mirrors `glances/plugins/version/__init__.py`. Surfaces:
//!   - `glances_version`: semver string for this port (from Cargo.toml)
//!   - `api_version`:     protocol version for the REST / MCP surface
//!   - `crustacean`:      the language + std-only promise ("rust")
//!   - `std`:             std-only marker ("true")
//!
//! All values are static; `update()` is a no-op.

use std::collections::BTreeMap;
use std::env;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "version";

/// Mirrors the `version` field in Cargo.toml. Hard-coded here so the
/// plugin doesn't need build-script plumbing (matches the std-only
/// constraint of the rest of the project).
pub const GLANCES_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Protocol version for the HTTP API / MCP server. Bumped on breaking
/// changes; consumers can feature-detect against this string.
pub const API_VERSION: &str = "4";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(VersionPlugin::new()));
}

/// Build the static stats payload. Public so tests can use
/// the same source of truth.
pub fn stats_payload() -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    m.insert("glances_version".into(), Value::String(GLANCES_VERSION.to_string()));
    m.insert("api_version".into(),     Value::String(API_VERSION.to_string()));
    m.insert("crustacean".into(),      Value::String("rust".to_string()));
    m.insert("std".into(),             Value::String("true".to_string()));
    // Mirror the Python plugin's `plugin_version` field as well.
    m.insert("plugin_version".into(),  Value::String(API_VERSION.to_string()));
    m
}

pub struct VersionPlugin { base: GlancesPluginModel }

impl Default for VersionPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Object(stats_payload())),
        }
    }
}

impl Plugin for VersionPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        // Static metadata; nothing to refresh.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_version() {
        let p = VersionPlugin::new();
        assert_eq!(p.name(), NAME);
        assert_eq!(p.name(), "version");
    }

    #[test]
    fn stats_contains_required_fields() {
        let obj = stats_payload();
        for k in ["glances_version", "api_version", "crustacean", "std", "plugin_version"] {
            assert!(obj.contains_key(k), "missing version field: {k}");
            assert!(matches!(obj[k], Value::String(_)),
                "version field {k} must be a string");
        }
    }

    #[test]
    fn values_are_well_formed() {
        let obj = stats_payload();
        assert_eq!(obj["glances_version"].as_str().unwrap(), GLANCES_VERSION);
        assert_eq!(obj["api_version"].as_str().unwrap(), API_VERSION);
        assert_eq!(obj["crustacean"].as_str().unwrap(), "rust");
        assert_eq!(obj["std"].as_str().unwrap(), "true");
        // Non-empty: a zero-string would be a packaging bug.
        assert!(!GLANCES_VERSION.is_empty(), "GLANCES_VERSION must be non-empty");
        assert!(!API_VERSION.is_empty(), "API_VERSION must be non-empty");
    }

    #[test]
    fn reset_restores_static_payload() {
        let mut p = VersionPlugin::new();
        p.stats_mut().as_object_mut().unwrap()
            .insert("glances_version".into(), Value::String("overridden".into()));
        p.reset();
        let obj = p.stats().as_object().unwrap();
        assert_eq!(obj["glances_version"].as_str().unwrap(), GLANCES_VERSION);
    }

    #[test]
    fn update_is_idempotent() {
        let mut p = VersionPlugin::new();
        let before = p.stats().as_object().unwrap().clone();
        p.update().expect("update should not fail");
        assert_eq!(p.stats().as_object().unwrap(), &before);
    }
}