//! Tests for the amps plugin — M11 stub only. The real AMP wiring
//! lands in M12 (config iteration, argv-only subprocess, regex against
//! the process list). For M11 we assert the wire shape: the plugin
//! registers, its initial and post-update stats are empty arrays.

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::plugins::amps::{self, AmpsPlugin, NAME};

#[test]
fn name_and_register() {
    let s = GlancesStats::new(1.0);
    amps::register(&s);
    assert!(s.plugin_names().contains(&NAME));
}

#[test]
fn initial_stats_is_empty_array() {
    let p = AmpsPlugin::new();
    let arr = p.stats().as_array().expect("initial stats is Value::Array");
    assert!(arr.is_empty(), "M11 stub starts with empty array");
}

#[test]
fn update_leaves_stats_as_empty_array() {
    let mut p = AmpsPlugin::new();
    p.update().expect("update must not error");
    let arr = p.stats().as_array().expect("stats remains Value::Array");
    assert!(arr.is_empty(), "M11 stub always emits empty array");
}

#[test]
fn reset_restores_initial_empty_array() {
    let mut p = AmpsPlugin::new();
    p.update().unwrap();
    p.reset();
    let arr = p.stats().as_array().expect("stats is array after reset");
    assert!(arr.is_empty());
}

#[test]
fn get_key_is_name() {
    let p = AmpsPlugin::new();
    assert_eq!(p.get_key(), Some("name"));
}

#[test]
fn default_impl_matches_new() {
    let a = AmpsPlugin::new();
    let b = AmpsPlugin::default();
    assert_eq!(a.stats(), b.stats());
}
