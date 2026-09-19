//! INI-style config parser + per-OS config-dir resolution.
//!
//! M1 stub: parses a flat INI with `[section]` headers and `key=value` lines.
//! Full per-OS config-dir lookup (Linux ~/.config/glances/, macOS
//! ~/Library/Application Support/, Windows %APPDATA%) lands in M2.

use std::collections::BTreeMap;

use super::error::{GlancesError, Result};

/// Parsed config: section -> key -> value.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub sections: BTreeMap<String, BTreeMap<String, String>>,
}

impl Config {
    pub fn empty() -> Self { Self::default() }

    pub fn parse(text: &str) -> Result<Self> {
        let mut cfg = Self::default();
        let mut section = String::from("default");
        for (lineno, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') { continue; }
            if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
                section = line[1..line.len() - 1].trim().to_string();
                cfg.sections.entry(section.clone()).or_insert_with(BTreeMap::new);
                continue;
            }
            let eq = match line.find('=') {
                Some(p) => p,
                None => return Err(GlancesError::Parse(format!("line {}: missing '='", lineno + 1))),
            };
            let key = line[..eq].trim().to_string();
            let value = line[eq + 1..].trim().to_string();
            cfg.sections.entry(section.clone()).or_insert_with(BTreeMap::new).insert(key, value);
        }
        Ok(cfg)
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections.get(section).and_then(|s| s.get(key)).map(|s| s.as_str())
    }

    pub fn get_float(&self, section: &str, key: &str) -> Option<f64> {
        self.get(section, key).and_then(|v| v.parse::<f64>().ok())
    }

    pub fn section(&self, section: &str) -> Option<&BTreeMap<String, String>> {
        self.sections.get(section)
    }

    /// Load from a file path. Missing file is an error.
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(GlancesError::Io)?;
        Self::parse(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_minimal() {
        let c = Config::parse("[cpu]\nuser_careful = 50\nuser_warning = 70\n").unwrap();
        assert_eq!(c.get("cpu", "user_careful"), Some("50"));
        assert_eq!(c.get_float("cpu", "user_careful"), Some(50.0));
    }
    #[test]
    fn parse_comments_and_blank_lines() {
        let c = Config::parse("# comment\n\n[global]\nrefresh = 2\n").unwrap();
        assert_eq!(c.get("global", "refresh"), Some("2"));
    }
    #[test]
    fn parse_missing_equals_errors() {
        assert!(Config::parse("[bad]\nkey_no_value\n").is_err());
    }
}
