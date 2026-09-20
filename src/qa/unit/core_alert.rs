//! Conformance tests for the alert pipeline (upstream `get_alert` /
//! `update_views` / default-limits parity).

use std::collections::BTreeMap;

use crate::core::events::EventLog;
use crate::core::alerts::LimitValue;
use crate::core::plugin::{FieldDesc, FieldFlags, GlancesPluginModel, Unit};
use crate::core::value::Value;

fn model_with_limits(plugin: &'static str, limits: &[(&str, f64)]) -> GlancesPluginModel {
    let mut m = GlancesPluginModel::new(plugin, Value::Object(BTreeMap::new()));
    for (k, v) in limits {
        m.limits.insert((*k).to_string(), LimitValue::Float(*v));
    }
    m
}

fn no_events() -> Option<&'static mut EventLog> {
    None
}

#[test]
fn alert_chain_walks_careful_warning_critical() {
    let mut m = model_with_limits("cpu", &[("cpu_user_careful", 50.0), ("cpu_user_warning", 70.0), ("cpu_user_critical", 90.0)]);
    assert_eq!(m.get_alert(30.0, 0.0, 100.0, "user", None, false, true, None, no_events()), "OK");
    assert_eq!(m.get_alert(60.0, 0.0, 100.0, "user", None, false, true, None, no_events()), "CAREFUL");
    assert_eq!(m.get_alert(80.0, 0.0, 100.0, "user", None, false, true, None, no_events()), "WARNING");
    assert_eq!(m.get_alert(95.0, 0.0, 100.0, "user", None, false, true, None, no_events()), "CRITICAL");
    // Trigger is tracked (manage_threshold parity).
    assert_eq!(m.thresholds.get("cpu_user").map(String::as_str), Some("CRITICAL"));
}

#[test]
fn alert_defaults_to_default_without_limits_or_value() {
    let mut m = model_with_limits("cpu", &[]);
    assert_eq!(m.get_alert(95.0, 0.0, 100.0, "user", None, false, true, None, no_events()), "DEFAULT");
    let mut m2 = model_with_limits("cpu", &[("cpu_user_critical", 90.0)]);
    assert_eq!(m2.get_alert(0.0, 0.0, 100.0, "user", None, false, false, None, no_events()), "DEFAULT");
    assert_eq!(m2.get_alert(50.0, 0.0, 0.0, "user", None, false, true, None, no_events()), "DEFAULT");
}

#[test]
fn stat_limit_beats_plugin_limit() {
    let mut m = model_with_limits(
        "cpu",
        &[("cpu_careful", 10.0), ("cpu_user_careful", 90.0)],
    );
    // Stat-level 90 wins over plugin-level 10 → still OK at 50.
    assert_eq!(m.get_alert(50.0, 0.0, 100.0, "user", None, false, true, None, no_events()), "OK");
}

#[test]
fn minimum_forces_careful_and_is_max_never_survives() {
    let mut m = model_with_limits("cpu", &[("cpu_user_careful", 50.0)]);
    assert_eq!(m.get_alert(50.0, 60.0, 100.0, "user", None, false, true, None, no_events()), "CAREFUL");
    // Upstream truth: the initial MAX is overwritten by the chain in
    // every path (no limits → DEFAULT here).
    let mut m2 = model_with_limits("cpu", &[]);
    assert_eq!(m2.get_alert(10.0, 0.0, 100.0, "user", None, true, true, None, no_events()), "DEFAULT");
}

#[test]
fn log_tag_appends_suffix_and_records_event() {
    let mut m = model_with_limits("cpu", &[("cpu_user_critical", 90.0)]);
    m.limits.insert("cpu_user_log".into(), LimitValue::List(vec!["true".into()]));
    let mut log = EventLog::default();
    let d = m.get_alert(95.0, 0.0, 100.0, "user", None, false, true, None, Some(&mut log));
    assert_eq!(d, "CRITICAL_LOG");
    let snap = log.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].stat, "cpu_user");
}

#[test]
fn build_views_decorates_flagged_fields() {
    const DESCS: &[FieldDesc] = &[
        FieldDesc { name: "total", unit: Unit::Percent, flags: FieldFlags::LOG },
        FieldDesc { name: "steal", unit: Unit::Percent, flags: FieldFlags::ALERT },
    ];
    let mut stats = BTreeMap::new();
    stats.insert("total".into(), Value::Float(95.0));
    stats.insert("steal".into(), Value::Float(80.0));
    stats.insert("idle".into(), Value::Float(5.0));
    let mut m = GlancesPluginModel::new("cpu", Value::Object(stats.clone()));
    m.stats = Value::Object(stats);
    m.limits.insert("cpu_total_critical".into(), LimitValue::Float(90.0));
    m.limits.insert("cpu_total_log".into(), LimitValue::List(vec!["true".into()]));
    m.limits.insert("cpu_steal_warning".into(), LimitValue::Float(70.0));
    let mut log = EventLog::default();
    m.build_views(DESCS, None, Some(&mut log));
    let elem = m.views.get("").expect("scalar views");
    assert_eq!(elem.get("total").map(String::as_str), Some("CRITICAL_LOG"));
    assert_eq!(elem.get("steal").map(String::as_str), Some("WARNING"));
    assert_eq!(elem.get("idle").map(String::as_str), Some("DEFAULT"));
    assert!(!log.snapshot().is_empty(), "log field must record an event");
}

#[test]
fn default_limits_fill_missing_without_clobbering_user_keys() {
    use crate::core::alerts::default_limit_entries;
    let mut m = GlancesPluginModel::new("load", Value::Object(BTreeMap::new()));
    // As `apply_limits_config` stores it: plugin-prefixed.
    m.limits.insert("load_careful".into(), LimitValue::Float(1.5));
    m.apply_default_limits(&default_limit_entries("load"), 4);
    // User key wins; missing siblings fill from built-ins.
    assert_eq!(m.limits.get("load_careful"), Some(&LimitValue::Float(1.5)));
    assert_eq!(m.limits.get("load_warning"), Some(&LimitValue::Float(1.0)));
    assert_eq!(m.limits.get("load_critical"), Some(&LimitValue::Float(5.0)));
}

#[test]
fn cpu_ctx_switches_defaults_scale_with_cores() {
    use crate::core::alerts::default_limit_entries;
    let mut m = GlancesPluginModel::new("cpu", Value::Object(BTreeMap::new()));
    m.apply_default_limits(&default_limit_entries("cpu"), 8);
    // Upstream #1212: base = 500000 * 10% * ncpu.
    assert_eq!(m.limits.get("cpu_ctx_switches_critical"), Some(&LimitValue::Float(400000.0)));
    assert_eq!(m.limits.get("cpu_user_careful"), Some(&LimitValue::Float(50.0)));
}
