//! CLI flag dispatch tests — verify each flag round-trips correctly.

use crate::cli::args::{parse_args_with, Mode, SnmpVersion};

fn run(argv: &[&str]) -> crate::cli::args::Args {
    let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    parse_args_with(&owned)
}

#[test]
fn default_args_have_standard_ports() {
    let a = crate::cli::args::Args::default();
    assert_eq!(a.web_port, 61208);
    assert_eq!(a.bind_address, "0.0.0.0");
    assert_eq!(a.refresh_time, 2.0);
}

#[test]
fn short_flag_combinations() {
    // -w sets web mode.
    let a = run(&["-w"]);
    assert_eq!(a.mode, Mode::WebServer);
    // -h sets help mode.
    let a = run(&["-h"]);
    assert_eq!(a.mode, Mode::Help);
    // -V sets version mode.
    let a = run(&["-V"]);
    assert_eq!(a.mode, Mode::Version);
}

#[test]
fn long_flag_aliases_match_short() {
    assert_eq!(run(&["--webserver"]).mode, Mode::WebServer);
    assert_eq!(run(&["--help"]).mode, Mode::Help);
    assert_eq!(run(&["--version"]).mode, Mode::Version);
}

#[test]
fn refresh_time_parses() {
    let a = run(&["-t", "5"]);
    assert_eq!(a.refresh_time, 5.0);
    let a = run(&["--time", "0.5"]);
    assert!((a.refresh_time - 0.5).abs() < 1e-6);
}

#[test]
fn client_flag_sets_client() {
    let a = run(&["-c", "192.168.1.1"]);
    assert_eq!(a.client_host, Some("192.168.1.1".to_string()));
    assert_eq!(a.mode, Mode::Client);
}

#[test]
fn removed_port_flag_is_ignored() {
    // -p died with the XML-RPC server: unknown flags never flip modes.
    let a = run(&["-p", "8080"]);
    assert_eq!(a.mode, Mode::Standalone);
}

#[test]
fn bind_sets_address() {
    let a = run(&["-B", "127.0.0.1"]);
    assert_eq!(a.bind_address, "127.0.0.1");
}

#[test]
fn username_and_password() {
    // Upstream: `-u` takes the name, bare `--username`/`--password`
    // prompt on stdin (no valued form — argv leaks through ps).
    let a = run(&["-u", "admin"]);
    assert_eq!(a.username_used.as_deref(), Some("admin"));
    assert!(!a.username_prompt);
    let b = run(&["--username"]);
    assert!(b.username_prompt);
    assert!(!b.password_prompt);
    let c = run(&["--password"]);
    assert!(c.password_prompt);
}

#[test]
fn disable_flags() {
    let a = run(&["--disable-history", "--disable-webui", "--disable-config-exec"]);
    assert!(a.disable_history);
    assert!(a.disable_webui);
    assert!(a.disable_config_exec);
}

#[test]
fn display_subsets_stay_distinct() {
    // Each of -2/-3/-4/-5/--light keeps its own upstream meaning
    // (upstream main.py init_ui_mode); none collapses into another.
    let a = run(&["-2"]);
    assert!(a.disable_left_sidebar && !a.disable_quicklook && !a.full_quicklook && !a.disable_top && !a.light);
    let b = run(&["-3"]);
    assert!(b.disable_quicklook && !b.disable_left_sidebar && !b.full_quicklook && !b.disable_top && !b.light);
    let c = run(&["-4"]);
    assert!(c.full_quicklook && !c.disable_left_sidebar && !c.disable_quicklook && !c.disable_top && !c.light);
    let d = run(&["-5"]);
    assert!(d.disable_top && !d.disable_left_sidebar && !d.disable_quicklook && !d.full_quicklook && !d.light);
    let e = run(&["--light"]);
    assert!(e.light && !e.disable_left_sidebar && !e.disable_quicklook && !e.full_quicklook && !e.disable_top);
    // Long-form aliases.
    assert!(run(&["--disable-left-sidebar"]).disable_left_sidebar);
    assert!(run(&["--disable-quicklook"]).disable_quicklook);
    assert!(run(&["--full-quicklook"]).full_quicklook);
    assert!(run(&["--disable-top"]).disable_top);
    assert!(run(&["--enable-light"]).light);
}

#[test]
fn fs_free_space_flag() {
    assert!(run(&["--fs-free-space"]).fs_free_space);
    assert!(!run(&[] as &[&str]).fs_free_space);
}

#[test]
fn snmp_version_flag() {
    assert_eq!(run(&["--snmp-version", "1"]).snmp_version, SnmpVersion::V1);
    assert_eq!(run(&["--snmp-version", "2c"]).snmp_version, SnmpVersion::V2c);
    assert_eq!(run(&["--snmp-version", "3"]).snmp_version, SnmpVersion::V3);
    // Unknown version defaults to V2c (matches Python Glances).
    assert_eq!(run(&["--snmp-version", "garbage"]).snmp_version, SnmpVersion::V2c);
}

#[test]
fn url_prefix_empty_by_default() {
    let a = crate::cli::args::Args::default();
    assert_eq!(a.url_prefix, "");
    let a = run(&["--url-prefix", "/glances"]);
    assert_eq!(a.url_prefix, "/glances");
}

#[test]
fn stop_after_parses() {
    let a = run(&["--stop-after", "100"]);
    assert_eq!(a.stop_after, Some(100));
}

#[test]
fn unknown_flag_ignored() {
    // Unknown flag must not crash; args remain at defaults.
    // Note: passing a value with the unknown flag would mark it as
    // positional, so test with just the bare flag.
    let a = run(&["--definitely-not-a-flag"]);
    assert_eq!(a.refresh_time, 2.0);
    assert_eq!(a.mode, Mode::Standalone);
}

#[test]
fn config_path_set() {
    let a = run(&["-C", "/etc/glances/glances.conf"]);
    assert_eq!(a.config_path.as_deref(), Some("/etc/glances/glances.conf"));
}

#[test]
fn debug_flag() {
    let a = run(&["-d"]);
    assert!(a.debug);
    let a = run(&["--debug"]);
    assert!(a.debug);
}

#[test]
fn quiet_flag() {
    let a = run(&["-q"]);
    assert!(a.quiet);
    let a = run(&["--quiet"]);
    assert!(a.quiet);
}

#[test]
fn auth_enabled_flag() {
    let a = run(&["--auth-enabled"]);
    assert!(a.auth_enabled);
}

#[test]
fn access_log_and_ping_flags() {
    let a = run(&["--access-log"]);
    assert!(a.access_log);
    assert!(!crate::cli::args::Args::default().access_log);
    let a = run(&["--ping", "127.0.0.1:61208"]);
    assert_eq!(a.mode, Mode::Ping);
    assert_eq!(a.ping_target.as_deref(), Some("127.0.0.1:61208"));
}
