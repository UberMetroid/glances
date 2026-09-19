//! Edge cases: empty inputs to public APIs.

use crate::core::config::Config;
use crate::core::filter::ProcessFilter;
use crate::core::history::GlancesHistory;
use crate::core::value::Value;

#[test]
fn value_null_roundtrip() {
    let v = Value::Null;
    assert_eq!(crate::core::value::to_json(&v), "null");
}

#[test]
fn config_empty_string() {
    let c = Config::parse("").unwrap();
    assert!(c.sections.is_empty());
}

#[test]
fn config_only_comments() {
    let c = Config::parse("# nothing here\n# really\n").unwrap();
    assert!(c.sections.is_empty());
}

#[test]
fn filter_empty_string_is_empty() {
    let f = ProcessFilter::new("").unwrap();
    assert!(!f.is_active());
    assert!(f.matches("anything", "anywhere"));
}

#[test]
fn history_zero_capacity_keeps_zero() {
    let mut h = GlancesHistory::with_capacity(0);
    h.add("x", 1.0);
    assert!(h.get("x", 0).is_empty());
}

#[test]
fn history_unicode_value_doesnt_panic() {
    let mut h = GlancesHistory::new();
    h.add("emoji_🚀", f64::NAN);
    h.add("rtl_عربي", 1.0);
    assert_eq!(h.get("emoji_🚀", 0).len(), 1);
}
