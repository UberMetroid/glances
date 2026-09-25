//! Help plugin — key bindings shown by the help screen.
//!
//! Mirrors `glances/plugins/help/__init__.py`. Each binding maps a key
//! (or key combo) to a short human-readable description of what it does.
//!
//! Ships the default key-binding map (upstream Curses UI bindings)
//! as data for API consumers.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "help";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(HelpPlugin::new()));
}

/// Default key bindings. Centralised so the binary's CLI help text and
/// the plugin's stats stay in sync.
pub fn default_bindings() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("a".into(),  "Sort processes automatically".into());
    m.insert("b".into(),  "Bit/s or Byte/s for network/disk I/O".into());
    m.insert("c".into(),  "Sort processes by CPU%".into());
    m.insert("d".into(),  "Show/hide disk I/O stats".into());
    m.insert("e".into(),  "Show/hide the top extended stats panel".into());
    m.insert("f".into(),  "Show/hide filesystem stats".into());
    m.insert("g".into(),  "Generate history graphs".into());
    m.insert("h".into(),  "Show/hide this help screen".into());
    m.insert("i".into(),  "Sort processes by I/O rate".into());
    m.insert("l".into(),  "Show/hide log messages".into());
    m.insert("m".into(),  "Sort processes by MEM%".into());
    m.insert("n".into(),  "Show/hide network stats".into());
    m.insert("p".into(),  "Sort processes by name".into());
    m.insert("q".into(),  "Quit (also Esc)".into());
    m.insert("s".into(),  "Show/hide sensors stats".into());
    m.insert("t".into(),  "View network I/O as combo".into());
    m.insert("u".into(),  "View cumulative network I/O".into());
    m.insert("w".into(),  "Delete finished warning alerts".into());
    m.insert("x".into(),  "Delete finished critical alerts".into());
    m.insert("y".into(),  "Show/hide hddtemp stats".into());
    m.insert("z".into(),  "Show/hide process stats".into());
    m.insert("1".into(),  "Global CPU stats or per-CPU stats".into());
    m.insert("2".into(),  "Show left sidebar or right sidebar".into());
    m.insert("3".into(),  "Enable/disable the quicklook plugin".into());
    m.insert("4".into(),  "Enable/disable the memory plugin".into());
    m.insert("5".into(),  "Enable/disable the swap plugin".into());
    m.insert("/".into(),  "Enable/disable the process filter".into());
    m.insert("ENTER".into(), "Edit the process filter".into());
    m
}

pub struct HelpPlugin { base: GlancesPluginModel }

impl Default for HelpPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpPlugin {
    pub fn new() -> Self {
        // Bindings are static — populated once in `new()` so consumers can
        // read them without ever calling `update()`.
        let mut m: BTreeMap<String, Value> = BTreeMap::new();
        for (k, v) in default_bindings() {
            m.insert(k, Value::String(v));
        }
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
    fn update(&mut self) -> Result<()> {
        // Bindings are static; nothing to refresh.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_help() {
        let p = HelpPlugin::new();
        assert_eq!(p.name(), NAME);
        assert_eq!(p.name(), "help");
    }

    #[test]
    fn stats_is_an_object_of_bindings() {
        let p = HelpPlugin::new();
        let obj = p.stats().as_object().expect("stats must be an object");
        assert!(!obj.is_empty(), "help bindings must not be empty");
        // Every value must be a string description.
        for (k, v) in obj {
            assert!(matches!(v, Value::String(_)), "binding {k} must be a string");
        }
    }

    #[test]
    fn well_known_keys_are_present() {
        let p = HelpPlugin::new();
        let obj = p.stats().as_object().unwrap();
        for k in ["h", "q", "c", "m", "a"] {
            assert!(obj.contains_key(k), "missing well-known binding: {k}");
            assert!(obj[k].as_str().unwrap().contains(' '),
                "binding {k} should be a description, not a single char");
        }
    }

    #[test]
    fn default_bindings_match_new_plugin() {
        let defaults = default_bindings();
        let p = HelpPlugin::new();
        let obj = p.stats().as_object().unwrap();
        assert_eq!(defaults.len(), obj.len(),
            "default_bindings count must match the plugin's stats count");
        for (k, v) in defaults {
            assert_eq!(obj.get(&k).and_then(Value::as_str).map(str::to_string),
                Some(v), "binding {k} differs from default_bindings()");
        }
    }

    #[test]
    fn update_is_a_no_op() {
        let mut p = HelpPlugin::new();
        let before = p.stats().as_object().unwrap().len();
        p.update().expect("update should not fail");
        assert_eq!(p.stats().as_object().unwrap().len(), before);
    }
}