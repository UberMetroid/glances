//! Help plugin — the default key-binding map as data.
//!
//! Static table (key → description) for API consumers. The binary's
//! CLI help text draws from the same bindings so the two never drift.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "help";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(HelpPlugin::new()));
}

/// The default bindings. Frozen user-visible data.
pub fn default_bindings() -> BTreeMap<String, String> {
    [
        ("a", "Sort processes automatically"),
        ("b", "Bit/s or Byte/s for network/disk I/O"),
        ("c", "Sort processes by CPU%"),
        ("d", "Show/hide disk I/O stats"),
        ("e", "Show/hide the top extended stats panel"),
        ("f", "Show/hide filesystem stats"),
        ("g", "Generate history graphs"),
        ("h", "Show/hide this help screen"),
        ("i", "Sort processes by I/O rate"),
        ("l", "Show/hide log messages"),
        ("m", "Sort processes by MEM%"),
        ("n", "Show/hide network stats"),
        ("p", "Sort processes by name"),
        ("q", "Quit (also Esc)"),
        ("s", "Show/hide sensors stats"),
        ("t", "View network I/O as combo"),
        ("u", "View cumulative network I/O"),
        ("w", "Delete finished warning alerts"),
        ("x", "Delete finished critical alerts"),
        ("y", "Show/hide hddtemp stats"),
        ("z", "Show/hide process stats"),
        ("1", "Global CPU stats or per-CPU stats"),
        ("2", "Show left sidebar or right sidebar"),
        ("3", "Enable/disable the quicklook plugin"),
        ("4", "Enable/disable the memory plugin"),
        ("5", "Enable/disable the swap plugin"),
        ("/", "Enable/disable the process filter"),
        ("ENTER", "Edit the process filter"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub struct HelpPlugin { base: GlancesPluginModel }

impl Default for HelpPlugin {
    fn default() -> Self { Self::new() }
}

impl HelpPlugin {
    pub fn new() -> Self {
        let m: BTreeMap<String, Value> =
            default_bindings().into_iter().map(|(k, v)| (k, Value::String(v))).collect();
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

impl Plugin for HelpPlugin {
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
        assert_eq!(HelpPlugin::new().name(), "help");
    }
    #[test]
    fn all_values_are_descriptions() {
        let obj = HelpPlugin::new().stats().clone();
        let obj = obj.as_object().unwrap();
        assert_eq!(obj.len(), 28);
        for (k, v) in obj {
            let s = v.as_str().unwrap_or_else(|| panic!("{k} must be a string"));
            assert!(s.contains(' '), "{k} should read as a description");
        }
    }
    #[test]
    fn stats_match_the_default_table() {
        let obj = HelpPlugin::new().stats().clone();
        let obj = obj.as_object().unwrap();
        for (k, v) in default_bindings() {
            assert_eq!(obj.get(&k).and_then(Value::as_str), Some(v.as_str()));
        }
    }
    #[test]
    fn update_changes_nothing() {
        let mut p = HelpPlugin::new();
        let before = p.stats().clone();
        p.update().unwrap();
        assert_eq!(p.stats(), &before);
    }
}
