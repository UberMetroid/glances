//! Rule and rule-list behavior: fullmatch semantics, key targeting,
//! first-colon splits, cmdline handling, and replace-on-set lists.

use std::collections::BTreeMap;

use crate::core::filter::{GlancesFilter, GlancesFilterList};
use crate::core::value::Value;

fn process(fields: &[(&str, Value)]) -> BTreeMap<String, Value> {
    fields.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

fn named(name: &str, argv: &[&str]) -> BTreeMap<String, Value> {
    process(&[
        ("name", Value::String(name.into())),
        ("cmdline", Value::Array(argv.iter().map(|a| Value::String((*a).into())).collect())),
    ])
}

fn rule(text: &str) -> GlancesFilter {
    let mut f = GlancesFilter::new();
    f.set_filter(Some(text));
    f
}

#[test]
fn bare_pattern_needs_a_fullmatch() {
    let f = rule("python");
    assert!(f.is_filtered(&named("python", &["python"])));
    assert!(!f.is_filtered(&named("python is in the place", &["x"])));
    let wide = rule(".*python.*");
    assert!(wide.is_filtered(&named("python is in the place", &["x"])));
    assert!(!wide.is_filtered(&named("snake is in the place", &["x"])));
}

#[test]
fn key_prefix_targets_one_field() {
    let f = rule("username:nicolargo");
    assert_eq!((f.key(), f.pattern()), (Some("username"), Some("nicolargo")));
    let hit = process(&[
        ("name", Value::String("snake".into())),
        ("username", Value::String("nicolargo".into())),
    ]);
    let miss = process(&[
        ("name", Value::String("snake".into())),
        ("username", Value::String("notme".into())),
    ]);
    assert!(f.is_filtered(&hit));
    assert!(!f.is_filtered(&miss));
    assert!(!f.is_filtered(&named("x", &["x"])));
}

#[test]
fn only_the_first_colon_splits() {
    let g = rule("cmdline:C:\\Prog");
    assert_eq!((g.key(), g.pattern()), (Some("cmdline"), Some("C:\\Prog")));
}

#[test]
fn keyless_rules_check_name_then_first_argv() {
    let f = rule(".*/firefox");
    assert!(f.is_filtered(&named("firefox", &["/usr/lib/firefox", "-child"])));
    assert!(f.is_filtered(&named("other", &["/usr/lib/firefox"])));
    assert!(!f.is_filtered(&named("other", &["/usr/bin/python"])));
    let second_only = rule("-child");
    assert!(!second_only.is_filtered(&named("other", &["/usr/lib/firefox", "-child"])));
}

#[test]
fn broken_pattern_disables_and_clear_resets() {
    let mut f = rule("(unclosed");
    assert!(!f.is_active());
    assert!(!f.is_filtered(&named("x", &["x"])));
    f.set_filter(None);
    assert!(!f.is_active());
    assert!(!f.is_filtered(&named("x", &["x"])));
}

#[test]
fn lists_or_and_replace_on_set() {
    let mut fl = GlancesFilterList::new();
    fl.set_filter(".*python.*,username:nicolargo");
    assert!(fl.is_filtered(&named("python is in the place", &["x"])));
    assert!(!fl.is_filtered(&named("snake is in the place", &["x"])));
    let user_hit = process(&[
        ("name", Value::String("snake is in the place".into())),
        ("username", Value::String("nicolargo".into())),
    ]);
    assert!(fl.is_filtered(&user_hit));
    fl.set_filter("username:nicolargo");
    assert!(!fl.is_filtered(&named("python is in the place", &["x"])));
    fl.clear();
    assert!(fl.is_empty());
}
