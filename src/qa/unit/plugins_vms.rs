//! Tests for the vms plugin — virsh/multipass parsers (fixture-based,
//! no live hypervisor required).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::plugins::vms::{
    collect_multipass, collect_virsh, parse_domstats, parse_multipass_csv, parse_virsh_list,
    register, row_to_value, VmRow,
};

const VIRSH_LIST_FIXTURE: &str = "\
 Id   Name   State
--------------------
 1    web01  running
 -    db01   shut off
";

const DOMSTATS_FIXTURE: &str = "\
Domain: 'web01'
  cpu.time=2000000000
  vcpu.current=2
  balloon.current=1048576
  balloon.maximum=2097152
Domain: 'db01'
  vcpu.current=4
";

const MULTIPASS_FIXTURE: &str = "\
Name,State,IPv4,Release
primary,Running,10.13.31.1,Ubuntu 22.04 LTS
build,Stopped,--,Ubuntu 24.04 LTS
";

#[test]
fn parse_virsh_list_reads_names_and_states() {
    let rows = parse_virsh_list(VIRSH_LIST_FIXTURE);
    assert_eq!(
        rows,
        vec![
            ("web01".to_string(), "running".to_string()),
            ("db01".to_string(), "shut off".to_string()),
        ]
    );
}

#[test]
fn parse_domstats_groups_by_domain() {
    let m = parse_domstats(DOMSTATS_FIXTURE);
    assert_eq!(m.len(), 2);
    assert_eq!(
        m["web01"].get("cpu.time").map(|s| s.as_str()),
        Some("2000000000")
    );
    assert_eq!(
        m["db01"].get("vcpu.current").map(|s| s.as_str()),
        Some("4")
    );
}

#[test]
fn parse_multipass_csv_reads_rows() {
    let rows = parse_multipass_csv(MULTIPASS_FIXTURE);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "primary");
    assert_eq!(rows[0].2, "10.13.31.1");
    assert_eq!(rows[1].1, "Stopped");
}

#[test]
fn row_to_value_omits_missing_engine_fields() {
    let row = VmRow {
        name: "m1".into(),
        status: "Running".into(),
        engine: "multipass".into(),
        engine_version: "1.0".into(),
        ipv4: Some("10.0.0.1 (Ubuntu)".into()),
        ..VmRow::default()
    };
    let v = row_to_value(&row);
    let obj = v.as_object().expect("object");
    assert_eq!(obj.get("name").and_then(|v| v.as_str()), Some("m1"));
    assert!(obj.contains_key("ipv4"));
    assert!(!obj.contains_key("cpu_time"), "no cpu data must omit key");
    assert!(!obj.contains_key("memory_usage"));
}

#[test]
fn collect_helpers_never_panic_without_binaries() {
    let mp_ver = OnceLock::new();
    let virsh_ver = OnceLock::new();
    let _ = collect_multipass(&mp_ver);
    let mut prev = HashMap::new();
    let _ = collect_virsh(&mut prev, &virsh_ver);
}

#[test]
fn register_plugin_appears_in_stats() {
    let s = crate::core::stats::GlancesStats::new(1.0);
    register(&s);
    assert!(s.plugin_names().contains(&"vms"));
}

#[test]
fn update_twice_is_stable_and_reset_clears() {
    use crate::core::plugin::Plugin;
    use crate::plugins::vms::VmsPlugin;
    let mut p = VmsPlugin::new();
    p.update().expect("update ok");
    let first = format!("{:?}", p.stats());
    p.update().expect("second update ok");
    assert_eq!(format!("{:?}", p.stats()), first, "cached tick must match");
    p.reset();
    assert!(p.stats().as_array().unwrap().is_empty());
}
