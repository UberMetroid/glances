//! Filter rules: one `key:pattern` rule plus comma-separated OR lists.
//!
//! A rule without a key fullmatches the process name or its first
//! cmdline word; `key:value` fullmatches that field instead (split on
//! the first colon only). Anything unmatchable — missing field,
//! non-string value, broken pattern — simply does not match.

use std::collections::BTreeMap;

use super::Regex;
use crate::core::value::Value;

/// A single rule: optional field key plus a compiled fullmatch pattern.
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

    /// Install a rule (`None` clears it). Patterns that fail to compile
    /// leave the rule disabled rather than erroring.
    pub fn set_filter(&mut self, value: Option<&str>) {
        self.input = value.map(str::to_string);
        self.regex = None;
        let Some(text) = value else {
            self.pattern = None;
            self.key = None;
            return;
        };
        match text.split_once(':') {
            None => {
                self.pattern = Some(text.to_string());
                self.key = None;
            }
            Some((k, p)) => {
                self.pattern = Some(p.to_string());
                self.key = Some(k.to_string());
            }
        }
        match self.pattern.clone().and_then(|p| Regex::compile(&p).ok()) {
            Some(r) => self.regex = Some(r),
            None => {
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

    /// Whether this rule selects the process.
    pub fn is_filtered(&self, process: &BTreeMap<String, Value>) -> bool {
        let Some(re) = &self.regex else { return false };
        match &self.key {
            None => {
                string_field_fullmatches(re, process, "name")
                    || cmdline_fullmatches(re, process)
            }
            Some(k) => string_field_fullmatches(re, process, k),
        }
    }
}

fn string_field_fullmatches(re: &Regex, process: &BTreeMap<String, Value>, key: &str) -> bool {
    matches!(process.get(key), Some(Value::String(s)) if re.is_full_match(s))
}

/// Cmdline argv lists match their first element only (an empty list
/// matches against ""); plain-string cmdlines match directly.
fn cmdline_fullmatches(re: &Regex, process: &BTreeMap<String, Value>) -> bool {
    match process.get("cmdline") {
        Some(Value::Array(items)) => match items.first() {
            Some(Value::String(s)) => re.is_full_match(s),
            Some(_) => false,
            None => re.is_full_match(""),
        },
        Some(Value::String(s)) => re.is_full_match(s),
        _ => false,
    }
}

/// OR list of rules. Installing a new value replaces the whole list —
/// narrowing or widening both come from the single latest string.
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
        self.filters.iter().filter_map(|f| f.input().map(str::to_string)).collect()
    }

    /// Whether any rule selects the process.
    pub fn is_filtered(&self, process: &BTreeMap<String, Value>) -> bool {
        self.filters.iter().any(|f| f.is_filtered(process))
    }
}
