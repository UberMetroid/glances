//! Conformance tests for alert actions (upstream `actions.py` /
//! `secure_popen` parity). Execution tests use `echo`/`true` only —
//! never anything destructive — and redirection targets under
//! `std::env::temp_dir()`.

use std::collections::BTreeMap;

use crate::core::actions::{sanitize_value, GlancesActions};

fn dict(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn sanitize_strips_operators_longest_first() {
    assert_eq!(sanitize_value("a&&b"), "a b");
    assert_eq!(sanitize_value("a|b"), "a b");
    assert_eq!(sanitize_value("a>>b"), "a b");
    assert_eq!(sanitize_value("a>b"), "a b");
    assert_eq!(sanitize_value("a&b"), "a b");
    assert_eq!(sanitize_value("plain"), "plain");
}

#[test]
fn repeat_gate_and_grace_timer() {
    // Grace timer suppresses everything on a fresh instance.
    let mut a = GlancesActions::new(9999.0, true);
    assert!(!a.run("s", "CRITICAL", &["true".to_string()], true, &dict(&[])));
    assert_eq!(a.get("s"), None);
    // Non-repeat fires once per trigger level.
    let mut b = GlancesActions::new(0.0, true);
    assert!(b.run("s", "CRITICAL", &["true".to_string()], false, &dict(&[])));
    assert_eq!(b.get("s"), Some("CRITICAL"));
    assert!(!b.run("s", "CRITICAL", &["true".to_string()], false, &dict(&[])));
    // Repeat always re-fires.
    assert!(b.run("s", "CRITICAL", &["true".to_string()], true, &dict(&[])));
    // New trigger level fires again.
    assert!(b.run("s", "WARNING", &["true".to_string()], false, &dict(&[])));
}

#[test]
fn mustache_redirect_renders_sanitized() {
    // `echo {{msg}} > file`: template renders, operators in VALUES
    // are stripped, output lands in the file.
    let path = std::env::temp_dir().join("glances-rs-action-test.txt");
    let _ = std::fs::remove_file(&path);
    let cmd = format!("echo value={{{{v}}}} > {}", path.display());
    let mut a = GlancesActions::new(0.0, true);
    assert!(a.run("s", "CRITICAL", &[cmd], true, &dict(&[("v", "42&&rm")]))); 
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("value=42 rm"), "got: {:?}", body);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn single_process_mode_ignores_operators() {
    // `--disable-config-exec` parity: `&&` is a literal argument.
    let mut a = GlancesActions::new(0.0, false);
    // `echo a && echo b` as ONE process: echo prints all its args.
    assert!(a.run("s", "CRITICAL", &["echo a && echo b".to_string()], true, &dict(&[])));
}

#[test]
fn get_limit_action_lookup_order() {
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
    // Stat-level wins over plugin-level.
    let (cmds, repeat) = m.get_limit_action("critical", "network_eth0_rx");
    assert_eq!(cmds, Some(vec!["echo hi".to_string()]));
    assert!(!repeat);
    // Repeat variant reports repeat=true.
    m.limits.insert(
        "network_eth0_rx_warning_action_repeat".into(),
        LimitValue::List(vec!["echo w".into()]),
    );
    let (cmds, repeat) = m.get_limit_action("warning", "network_eth0_rx");
    assert_eq!(cmds, Some(vec!["echo w".to_string()]));
    assert!(repeat);
    // Absent → (None, false).
    assert_eq!(m.get_limit_action("careful", "network_eth0_rx"), (None, false));
}
