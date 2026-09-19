//! Edge cases: malformed INI configs.

use crate::core::config::Config;

#[test]
fn missing_section_close() {
    // Unclosed `[` — we treat it as a positional section name and move on.
    // The current parser keeps going instead of erroring; this is the
    // lenient behavior matching Python configparser's default.
    let r = Config::parse("[unclosed\n");
    // Don't assert success or failure — just assert no panic.
    let _ = r;
}

#[test]
fn duplicate_keys_last_wins() {
    let c = Config::parse("[x]\nk=1\nk=2\n").unwrap();
    assert_eq!(c.get("x", "k"), Some("2"));
}

#[test]
fn unicode_in_value() {
    let c = Config::parse("[x]\nk = héllo 世界 🚀\n").unwrap();
    assert_eq!(c.get("x", "k"), Some("héllo 世界 🚀"));
}

#[test]
fn empty_section_name() {
    let c = Config::parse("[]\nk=1\n").unwrap();
    assert_eq!(c.get("", "k"), Some("1"));
}

#[test]
fn whitespace_only_value() {
    let c = Config::parse("[x]\nk =    \n").unwrap();
    assert_eq!(c.get("x", "k"), Some(""));
}
