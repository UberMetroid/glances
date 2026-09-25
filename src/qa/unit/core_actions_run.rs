//! Command-runner behavior: operator stripping, repeat gating, grace
//! window, template redirects, and single-process mode. Execution tests
//! only ever run `echo`/`true`, with redirects under the temp dir.

use std::collections::BTreeMap;

use crate::core::actions::{sanitize_value, GlancesActions};

fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn values_lose_operators_longest_first() {
    assert_eq!(sanitize_value("a&&b"), "a b");
    assert_eq!(sanitize_value("a|b"), "a b");
    assert_eq!(sanitize_value("a>>b"), "a b");
    assert_eq!(sanitize_value("a>b"), "a b");
    assert_eq!(sanitize_value("a&b"), "a b");
    assert_eq!(sanitize_value("plain"), "plain");
}

#[test]
fn grace_suppresses_and_levels_gate_repeats() {
    let mut fresh = GlancesActions::new(9999.0, true);
    assert!(!fresh.run("s", "CRITICAL", &["true".to_string()], true, &vars(&[])));
    assert_eq!(fresh.get("s"), None);
    let mut a = GlancesActions::new(0.0, true);
    assert!(a.run("s", "CRITICAL", &["true".to_string()], false, &vars(&[])));
    assert_eq!(a.get("s"), Some("CRITICAL"));
    assert!(!a.run("s", "CRITICAL", &["true".to_string()], false, &vars(&[])));
    assert!(a.run("s", "CRITICAL", &["true".to_string()], true, &vars(&[])));
    assert!(a.run("s", "WARNING", &["true".to_string()], false, &vars(&[])));
}

#[test]
fn redirect_renders_a_sanitized_template() {
    let path = std::env::temp_dir().join("glances-rs-action-test.txt");
    let _ = std::fs::remove_file(&path);
    let cmd = format!("echo value={{{{v}}}} > {}", path.display());
    let mut a = GlancesActions::new(0.0, true);
    assert!(a.run("s", "CRITICAL", &[cmd], true, &vars(&[("v", "42&&rm")])));
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("value=42 rm"), "got: {body:?}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn single_process_mode_passes_operators_through() {
    let mut a = GlancesActions::new(0.0, false);
    assert!(a.run("s", "CRITICAL", &["echo a && echo b".to_string()], true, &vars(&[])));
}

#[test]
fn action_lookup_prefers_stat_over_plugin() {
    use crate::core::alerts::LimitValue;
    use crate::core::plugin::GlancesPluginModel;
    use crate::core::value::Value;
    let mut m = GlancesPluginModel::new("network", Value::Null);
    m.limits.insert(
        "network_eth0_rx_critical_action".into(),
        LimitValue::List(vec!["echo hi".into()]),
    );
    m.limits.insert(
        "network_critical_action".into(),
        LimitValue::List(vec!["echo fallback".into()]),
    );
    let (cmds, repeat) = m.get_limit_action("critical", "network_eth0_rx");
    assert_eq!(cmds, Some(vec!["echo hi".to_string()]));
    assert!(!repeat);
    m.limits.insert(
        "network_eth0_rx_warning_action_repeat".into(),
        LimitValue::List(vec!["echo w".into()]),
    );
    let (cmds, repeat) = m.get_limit_action("warning", "network_eth0_rx");
    assert_eq!(cmds, Some(vec!["echo w".to_string()]));
    assert!(repeat);
    assert_eq!(m.get_limit_action("careful", "network_eth0_rx"), (None, false));
}
