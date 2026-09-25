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

/// Write an executable stub answering from the module fixtures.
/// `fail` makes every call exit 1.
fn stub_virsh(dir: &std::path::Path, fail: bool) {
    let body = if fail {
        "#!/bin/sh\nexit 1\n".to_string()
    } else {
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"list\" ]; then\ncat <<'EOF'\n{VIRSH_LIST_FIXTURE}EOF\nelif [ \"$1\" = \"domstats\" ]; then\ncat <<'EOF2'\n{DOMSTATS_FIXTURE}EOF2\nelse\necho 8.0.0\nfi\n"
        )
    };
    write_stub(dir, "virsh", &body);
}

fn stub_multipass(dir: &std::path::Path, fail: bool) {
    let body = if fail {
        "#!/bin/sh\nexit 1\n".to_string()
    } else {
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"list\" ]; then\ncat <<'EOF'\n{MULTIPASS_FIXTURE}EOF\nelse\necho 'multipass 1.13.0'\nfi\n"
        )
    };
    write_stub(dir, "multipass", &body);
}

fn write_stub(dir: &std::path::Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn collect_virsh_end_to_end_rates_cpu_time() {
    // Full pipeline: lookup -> list + domstats -> parse. cpu% is a
    // rate, so tick 1 reports None and tick 2 reports 0.0 (same
    // fixture counters, real elapsed wall time).
    let tmp = crate::qa::harness::TempDir::new("virsh-stub");
    stub_virsh(tmp.path(), false);
    let _env = crate::qa::harness::HelperEnv::set(tmp.path());
    let ver = OnceLock::new();
    let mut prev = HashMap::new();
    let first = collect_virsh(&mut prev, &ver);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].name, "web01");
    assert_eq!(first[0].engine_version, "8.0.0");
    assert_eq!(first[0].cpu_percent, None);
    assert_eq!(first[0].memory_usage, Some(1048576 * 1024));
    assert_eq!(first[0].memory_total, Some(2097152 * 1024));
    let second = collect_virsh(&mut prev, &ver);
    assert_eq!(second[0].cpu_percent, Some(0.0));
    assert_eq!(second[1].name, "db01");
    assert_eq!(second[1].cpu_percent, None, "no cpu.time stays None");
}

#[test]
fn collect_multipass_end_to_end_through_stub() {
    let tmp = crate::qa::harness::TempDir::new("multipass-stub");
    stub_multipass(tmp.path(), false);
    let _env = crate::qa::harness::HelperEnv::set(tmp.path());
    let rows = collect_multipass(&OnceLock::new());
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].engine_version, "1.13.0");
    assert_eq!(rows[0].ipv4.as_deref(), Some("10.13.31.1 (Ubuntu 22.04 LTS)"));
    assert_eq!(rows[1].ipv4, None, "-- address omits the field");
}

#[test]
fn collect_helpers_empty_when_binaries_fail() {
    let tmp = crate::qa::harness::TempDir::new("vms-fail");
    stub_virsh(tmp.path(), true);
    stub_multipass(tmp.path(), true);
    let _env = crate::qa::harness::HelperEnv::set(tmp.path());
    assert!(collect_multipass(&OnceLock::new()).is_empty());
    assert!(collect_virsh(&mut HashMap::new(), &OnceLock::new()).is_empty());
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
