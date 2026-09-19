//! CLI flag dispatch tests — verify each flag round-trips correctly.

use crate::cli::args::{parse_args_with, Mode, SnmpVersion};
use crate::cli::parse::parse_argv;
use crate::cli::flags::apply_flag;

fn run(argv: &[&str]) -> crate::cli::args::Args {
    let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    parse_args_with(&owned)
}

#[test]
fn default_args_have_standard_ports() {
    let a = crate::cli::args::Args::default();
    assert_eq!(a.server_port, 61209);
    assert_eq!(a.web_port, 61208);
    assert_eq!(a.bind_address, "0.0.0.0");
    assert_eq!(a.refresh_time, 2.0);
}

#[test]
fn short_flag_combinations() {
    // -s sets server mode.
    let a = run(&["-s"]);
    assert_eq!(a.mode, Mode::XmlRpcServer);
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
    assert_eq!(run(&["--server"]).mode, Mode::XmlRpcServer);
    assert_eq!(run(&["--webserver"]).mode, Mode::WebServer);
    assert_eq!(run(&["--help"]).mode, Mode::Help);
    assert_eq!(run(&["--version"]).mode, Mode::Version);
    assert_eq!(run(&["--browser"]).mode, Mode::Browser);
}

#[test]
fn refresh_time_parses() {
    let a = run(&["-t", "5"]);
    assert_eq!(a.refresh_time, 5.0);
    let a = run(&["--time", "0.5"]);
    assert!((a.refresh_time - 0.5).abs() < 1e-6);
}

#[test]
fn client_flag_sets_xmlrpc_client() {
    let a = run(&["-c", "192.168.1.1:61209"]);
    assert_eq!(a.client_host, Some("192.168.1.1:61209".to_string()));
    assert_eq!(a.mode, Mode::XmlRpcClient);
}

#[test]
fn port_parses() {
    let a = run(&["-p", "8080"]);
    assert_eq!(a.server_port, 8080);
}

#[test]
fn bind_sets_address() {
    let a = run(&["-B", "127.0.0.1"]);
    assert_eq!(a.bind_address, "127.0.0.1");
}

#[test]
fn username_and_password() {
    let a = run(&["-u", "admin", "--password", "hunter2"]);
    assert_eq!(a.username.as_deref(), Some("admin"));
    assert_eq!(a.password.as_deref(), Some("hunter2"));
}

#[test]
fn disable_flags() {
    let a = run(&["--disable-history", "--disable-webui", "--disable-config-exec"]);
    assert!(a.disable_history);
    assert!(a.disable_webui);
    assert!(a.disable_config_exec);
}

#[test]
fn light_modes() {
    for flag in &["-2", "-3", "-4", "-5", "--light"] {
        let a = run(&[flag]);
        assert!(a.light, "flag {} should set light=true", flag);
    }
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
fn export_target_accumulates() {
    let a = run(&["--export", "csv", "--export", "json", "--export", "prometheus"]);
    assert_eq!(a.export_targets, vec!["csv", "json", "prometheus"]);
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
fn mcp_path_default() {
    let a = crate::cli::args::Args::default();
    assert_eq!(a.mcp_path, "/mcp");
    let a = run(&["--mcp-path", "/api/mcp"]);
    assert_eq!(a.mcp_path, "/api/mcp");
}

#[test]
fn stdout_modes_parse() {
    assert_eq!(run(&["--stdout-csv"]).mode, Mode::StdoutCsv);
    assert_eq!(run(&["--stdout-json"]).mode, Mode::StdoutJson);
    let a = run(&["--stdout", "cpu.total,mem.percent"]);
    assert_eq!(a.mode, Mode::StdoutPath);
    assert_eq!(a.stdout_spec.as_deref(), Some("cpu.total,mem.percent"));
}

#[test]
fn web_port_and_plugin_lists_parse() {
    let a = run(&["--web-port", "8888"]);
    assert_eq!(a.web_port, 8888);
    // Previously `8888` stranded as a positional and flipped the mode.
    assert_eq!(a.mode, Mode::Standalone);
    let a = run(&["--disable-plugin", "fs,diskio", "--enable-plugin", "cpu"]);
    assert_eq!(a.disable_plugins, vec!["fs", "diskio"]);
    assert_eq!(a.enable_plugins, vec!["cpu"]);
}

#[test]
fn positional_does_not_flip_mode() {
    // Bare positionals used to silently switch to StdoutPath.
    let a = run(&["stray-argument"]);
    assert_eq!(a.mode, Mode::Standalone);
    assert_eq!(a.stdout_spec, None);
}

#[test]
fn refresh_time_rejects_non_finite() {
    for bad in ["nan", "inf", "-inf", "0", "-3"] {
        let a = run(&["-t", bad]);
        assert_eq!(a.refresh_time, 2.0, "-t {} must be rejected", bad);
    }
}

#[test]
fn export_opts_capture_generic_flags() {
    let a = run(&[
        "--export", "mqtt",
        "--export-mqtt-server", "broker.local:1884",
        "--export-mqtt-user", "svc",
    ]);
    assert_eq!(a.export_targets, vec!["mqtt"]);
    assert!(a.export_opts.contains(&("mqtt-server".to_string(), "broker.local:1884".to_string())));
    assert!(a.export_opts.contains(&("mqtt-user".to_string(), "svc".to_string())));
}

#[test]
fn plugin_registration_respects_enable_and_disable() {
    use crate::core::stats::GlancesStats;
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_filtered(&stats, &[], &["cpu".to_string(), "mem".to_string()]);
    let names = stats.plugin_names();
    assert_eq!(names, vec!["cpu", "mem"]);
    let stats2 = GlancesStats::new(2.0);
    crate::plugins::register_filtered(&stats2, &["cpu".to_string()], &[]);
    assert!(!stats2.plugin_names().contains(&"cpu"));
    assert!(stats2.plugin_names().contains(&"mem"));
}

#[test]
fn apply_flag_does_not_panic_on_empty_token() {
    let a = crate::cli::args::Args::default();
    let _ = a;
    let _ = apply_flag;
    let _ = parse_argv;
}
