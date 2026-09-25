//! Flag parsing: modes, aliases, values, and display subsets.

use crate::cli::args::{parse_args_with, Mode, SnmpVersion};

fn run(argv: &[&str]) -> crate::cli::args::Args {
    parse_args_with(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn stock_defaults() {
    let a = crate::cli::args::Args::default();
    assert_eq!(a.web_port, 61208);
    assert_eq!(a.bind_address, "0.0.0.0");
    assert_eq!(a.refresh_time, 2.0);
    assert_eq!(a.mode, Mode::Standalone);
}

#[test]
fn mode_shorts() {
    assert_eq!(run(&["-w"]).mode, Mode::WebServer);
    assert_eq!(run(&["-h"]).mode, Mode::Help);
    assert_eq!(run(&["-V"]).mode, Mode::Version);
}

#[test]
fn mode_longs_match_shorts() {
    assert_eq!(run(&["--webserver"]).mode, Mode::WebServer);
    assert_eq!(run(&["--help"]).mode, Mode::Help);
    assert_eq!(run(&["--version"]).mode, Mode::Version);
}

#[test]
fn refresh_accepts_positive_finite() {
    assert_eq!(run(&["-t", "5"]).refresh_time, 5.0);
    assert!((run(&["--time", "0.5"]).refresh_time - 0.5).abs() < 1e-6);
}

#[test]
fn client_carries_its_host() {
    let a = run(&["-c", "192.168.1.1"]);
    assert_eq!(a.client_host.as_deref(), Some("192.168.1.1"));
    assert_eq!(a.mode, Mode::Client);
}

#[test]
fn dead_port_flag_changes_nothing() {
    // -p died with XML-RPC: unknown flags never flip modes.
    assert_eq!(run(&["-p", "8080"]).mode, Mode::Standalone);
}

#[test]
fn bind_takes_the_address() {
    assert_eq!(run(&["-B", "127.0.0.1"]).bind_address, "127.0.0.1");
}

#[test]
fn username_forms_stay_apart() {
    // `-u` takes a name; bare prompt flags never take argv values
    // (process lists leak).
    let a = run(&["-u", "admin"]);
    assert_eq!(a.username_used.as_deref(), Some("admin"));
    assert!(!a.username_prompt);
    assert!(run(&["--username"]).username_prompt);
    assert!(!run(&["--username"]).password_prompt);
    assert!(run(&["--password"]).password_prompt);
}

#[test]
fn feature_kill_switches() {
    let a = run(&["--disable-history", "--disable-webui", "--disable-config-exec"]);
    assert!(a.disable_history && a.disable_webui && a.disable_config_exec);
}

#[test]
fn display_subsets_never_overlap() {
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
    assert!(run(&["--disable-left-sidebar"]).disable_left_sidebar);
    assert!(run(&["--disable-quicklook"]).disable_quicklook);
    assert!(run(&["--full-quicklook"]).full_quicklook);
    assert!(run(&["--disable-top"]).disable_top);
    assert!(run(&["--enable-light"]).light);
}

#[test]
fn fs_free_space_defaults_off() {
    assert!(run(&["--fs-free-space"]).fs_free_space);
    assert!(!run(&[] as &[&str]).fs_free_space);
}

#[test]
fn snmp_versions_with_v2c_fallback() {
    assert_eq!(run(&["--snmp-version", "1"]).snmp_version, SnmpVersion::V1);
    assert_eq!(run(&["--snmp-version", "2c"]).snmp_version, SnmpVersion::V2c);
    assert_eq!(run(&["--snmp-version", "3"]).snmp_version, SnmpVersion::V3);
    assert_eq!(run(&["--snmp-version", "garbage"]).snmp_version, SnmpVersion::V2c);
}

#[test]
fn url_prefix_empty_until_set() {
    assert_eq!(crate::cli::args::Args::default().url_prefix, "");
    assert_eq!(run(&["--url-prefix", "/glances"]).url_prefix, "/glances");
}

#[test]
fn stop_after_counts_ticks() {
    assert_eq!(run(&["--stop-after", "100"]).stop_after, Some(100));
}

#[test]
fn unknown_bare_flag_is_inert() {
    let a = run(&["--definitely-not-a-flag"]);
    assert_eq!(a.refresh_time, 2.0);
    assert_eq!(a.mode, Mode::Standalone);
}

#[test]
fn config_short_form() {
    assert_eq!(
        run(&["-C", "/etc/glances/glances.conf"]).config_path.as_deref(),
        Some("/etc/glances/glances.conf")
    );
}

#[test]
fn debug_and_quiet_pairs() {
    assert!(run(&["-d"]).debug && run(&["--debug"]).debug);
    assert!(run(&["-q"]).quiet && run(&["--quiet"]).quiet);
}

#[test]
fn auth_and_access_flags() {
    assert!(run(&["--auth-enabled"]).auth_enabled);
    assert!(run(&["--access-log"]).access_log);
    assert!(!crate::cli::args::Args::default().access_log);
}

#[test]
fn ping_selects_mode_and_target() {
    let a = run(&["--ping", "127.0.0.1:61208"]);
    assert_eq!(a.mode, Mode::Ping);
    assert_eq!(a.ping_target.as_deref(), Some("127.0.0.1:61208"));
}
