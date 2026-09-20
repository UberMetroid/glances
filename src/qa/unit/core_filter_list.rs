//! Conformance tests for the process filter (upstream `filter.py`
//! doctest parity: fullmatch, `key:` targeting, OR lists).

use std::collections::BTreeMap;

use crate::core::filter::{GlancesFilter, GlancesFilterList};
use crate::core::value::Value;

fn proc(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    m
}

fn name_cmd(name: &str, argv: &[&str]) -> BTreeMap<String, Value> {
    proc(&[
        ("name", Value::String(name.into())),
        (
            "cmdline",
            Value::Array(argv.iter().map(|a| Value::String((*a).into())).collect()),
        ),
    ])
}

#[test]
fn fullmatch_not_partial() {
    // Upstream: 'python' fullmatches — it does NOT match a longer name.
    let mut f = GlancesFilter::new();
    f.set_filter(Some("python"));
    assert!(f.is_filtered(&name_cmd("python", &["python"])));
    assert!(!f.is_filtered(&name_cmd("python is in the place", &["x"])));
    let mut f2 = GlancesFilter::new();
    f2.set_filter(Some(".*python.*"));
    assert!(f2.is_filtered(&name_cmd("python is in the place", &["x"])));
    assert!(!f2.is_filtered(&name_cmd("snake is in the place", &["x"])));
}

#[test]
fn key_targeting_and_split_once() {
    // Upstream doctests: 'username:nicolargo' targets the user field.
    let mut f = GlancesFilter::new();
    f.set_filter(Some("username:nicolargo"));
    assert_eq!(f.key(), Some("username"));
    assert_eq!(f.pattern(), Some("nicolargo"));
    assert!(f.is_filtered(&proc(&[
        ("name", Value::String("snake".into())),
        ("username", Value::String("nicolargo".into())),
    ])));
    assert!(!f.is_filtered(&proc(&[
        ("name", Value::String("snake".into())),
        ("username", Value::String("notme".into())),
    ])));
    // Missing key never matches.
    assert!(!f.is_filtered(&proc(&[("name", Value::String("x".into()))])));
    // Split on FIRST colon only (Windows paths keep working).
    let mut g = GlancesFilter::new();
    g.set_filter(Some("cmdline:C:\\Prog"));
    assert_eq!(g.key(), Some("cmdline"));
    assert_eq!(g.pattern(), Some("C:\\Prog"));
}

#[test]
fn no_key_matches_name_or_first_argv() {
    // Upstream: cmdline lists match on the FIRST element.
    let mut f = GlancesFilter::new();
    f.set_filter(Some(".*/firefox"));
    assert!(f.is_filtered(&name_cmd("firefox", &["/usr/lib/firefox", "-child"])));
    // Name misses but first argv hits → filtered.
    assert!(f.is_filtered(&name_cmd("other", &["/usr/lib/firefox"])));
    // Neither hits → not filtered.
    assert!(!f.is_filtered(&name_cmd("other", &["/usr/bin/python"])));
    // Second argv element alone never matches.
    let mut g = GlancesFilter::new();
    g.set_filter(Some("-child"));
    assert!(!g.is_filtered(&name_cmd("other", &["/usr/lib/firefox", "-child"])));
}

#[test]
fn bad_pattern_disables_and_none_clears() {
    let mut f = GlancesFilter::new();
    f.set_filter(Some("(unclosed"));
    assert!(!f.is_active());
    assert!(!f.is_filtered(&name_cmd("x", &["x"])));
    f.set_filter(None);
    assert!(!f.is_active());
    assert!(!f.is_filtered(&name_cmd("x", &["x"])));
}

#[test]
fn list_is_or_and_setter_replaces() {
    // Upstream GlancesFilterList doctest.
    let mut fl = GlancesFilterList::new();
    fl.set_filter(".*python.*,username:nicolargo");
    assert!(fl.is_filtered(&name_cmd("python is in the place", &["x"])));
    assert!(!fl.is_filtered(&name_cmd("snake is in the place", &["x"])));
    assert!(fl.is_filtered(&proc(&[
        ("name", Value::String("snake is in the place".into())),
        ("username", Value::String("nicolargo".into())),
    ])));
    assert!(!fl.is_filtered(&proc(&[
        ("name", Value::String("snake is in the place".into())),
        ("username", Value::String("notme".into())),
    ])));
    // Setter replaces: the python rule is gone after reset.
    fl.set_filter("username:nicolargo");
    assert!(!fl.is_filtered(&name_cmd("python is in the place", &["x"])));
}
