//! Upstream `filter.py` parity: `GlancesFilter` (one `key:pattern`
//! rule, fullmatch semantics) and `GlancesFilterList` (comma-separated
//! OR list whose setter replaces, never appends).
//!
//! Matching rules mirror the Python exactly:
//! - no `key:` → fullmatch against `name` OR first argv element;
//! - `key:value` → fullmatch against that field (split on FIRST colon);
//! - missing key / non-string value / uncompilable pattern → no match
//!   (bad patterns disable the filter, like upstream).

use std::collections::BTreeMap;

use super::Regex;
use crate::core::value::Value;

/// One filter rule.
#[derive(Debug, Default)]
pub struct GlancesFilter {
    input: Option<String>,
    pattern: Option<String>,
    key: Option<String>,
    regex: Option<Regex>,
}

impl GlancesFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the rule (`None` clears it). A pattern that fails to compile
    /// disables the rule (upstream logs + clears).
    pub fn set_filter(&mut self, value: Option<&str>) {
        self.input = value.map(|s| s.to_string());
        match value {
            None => {
                self.pattern = None;
                self.key = None;
            }
            Some(v) => {
                let mut parts = v.splitn(2, ':');
                let first = parts.next().unwrap_or("");
                match parts.next() {
                    None => {
                        self.pattern = Some(first.to_string());
                        self.key = None;
                    }
                    Some(rest) => {
                        self.pattern = Some(rest.to_string());
                        self.key = Some(first.to_string());
                    }
                }
            }
        }
        self.regex = None;
        if let Some(p) = self.pattern.clone() {
            if let Ok(r) = Regex::compile(&p) {
                self.regex = Some(r);
            } else {
                self.pattern = None;
                self.key = None;
            }
        }
    }

    pub fn input(&self) -> Option<&str> {
        self.input.as_deref()
    }

    pub fn pattern(&self) -> Option<&str> {
        self.pattern.as_deref()
    }

    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub fn is_active(&self) -> bool {
        self.regex.is_some()
    }

    /// True when the process matches this rule.
    pub fn is_filtered(&self, process: &BTreeMap<String, Value>) -> bool {
        let re = match &self.regex {
            Some(r) => r,
            None => return false,
        };
        match &self.key {
            None => {
                field_matches(re, process, "name")
                    || cmdline_first_matches(re, process)
            }
            Some(k) => field_matches(re, process, k),
        }
    }
}

fn field_matches(re: &Regex, process: &BTreeMap<String, Value>, key: &str) -> bool {
    match process.get(key) {
        Some(Value::String(s)) => re.is_full_match(s),
        _ => false,
    }
}

/// `cmdline` is an argv list: upstream matches its FIRST element when
/// non-empty, the space-joined list otherwise.
fn cmdline_first_matches(re: &Regex, process: &BTreeMap<String, Value>) -> bool {
    match process.get("cmdline") {
        Some(Value::Array(items)) if !items.is_empty() => match &items[0] {
            Value::String(s) => re.is_full_match(s),
            _ => false,
        },
        Some(Value::Array(_)) => re.is_full_match(""),
        Some(Value::String(s)) => re.is_full_match(s),
        _ => false,
    }
}

/// Comma-separated OR list of rules. Setting replaces the whole list
/// (upstream precedence parity: CLI never widens config, it replaces).
#[derive(Debug, Default)]
pub struct GlancesFilterList {
    filters: Vec<GlancesFilter>,
}

impl GlancesFilterList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_filter(&mut self, value: &str) {
        self.filters.clear();
        for part in value.split(',') {
            let mut f = GlancesFilter::new();
            f.set_filter(Some(part));
            self.filters.push(f);
        }
    }

    pub fn clear(&mut self) {
        self.filters.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.filters.iter().all(|f| !f.is_active())
    }

    pub fn inputs(&self) -> Vec<String> {
        self.filters
            .iter()
            .filter_map(|f| f.input().map(|s| s.to_string()))
            .collect()
    }

    /// True when at least one rule matches.
    pub fn is_filtered(&self, process: &BTreeMap<String, Value>) -> bool {
        self.filters.iter().any(|f| f.is_filtered(process))
    }
}
