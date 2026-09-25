//! Independent oracle for the CLI: pinned defaults and last-wins
//! precedence, derived from the documented contract.

use crate::cli::args::{parse_args_with, Mode, SnmpVersion};

fn run(argv: &[&str]) -> crate::cli::args::Args {
    parse_args_with(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn load_bearing_defaults() {
    let a = crate::cli::args::Args::default();
    assert_eq!(a.mode, Mode::Standalone);
    assert_eq!((a.web_port, a.bind_address.as_str(), a.refresh_time), (61208, "0.0.0.0", 2.0));
    assert_eq!(a.mcp_path, "/mcp");
    assert_eq!((a.snmp_port, a.snmp_version), (161, SnmpVersion::V2c));
    assert!(a.enable_separator && a.process_short_name);
    assert!(a.url_prefix.is_empty() && a.strftime_format.is_empty());
    assert!(a.disable_plugins.is_empty() && a.enable_plugins.is_empty());
    assert!(a.stop_after.is_none() && a.config_path.is_none() && a.client_host.is_none());
}

#[test]
fn later_flags_override_earlier_ones() {
    assert_eq!(run(&["-w", "--fetch"]).mode, Mode::Fetch);
    assert_eq!(run(&["--fetch", "-w"]).mode, Mode::WebServer);
    assert_eq!(run(&["-t", "5", "-t", "9"]).refresh_time, 9.0);
    assert_eq!(run(&["--web-port", "1", "--web-port", "2"]).web_port, 2);
    assert_eq!(run(&["--snmp-version", "1", "--snmp-version", "3"]).snmp_version, SnmpVersion::V3);
    let a = run(&["--process-long-name", "--process-short-name"]);
    assert!(a.process_short_name);
}

#[test]
fn invalid_values_leave_defaults() {
    assert_eq!(run(&["--web-port", "abc"]).web_port, 61208);
    assert_eq!(run(&["--stop-after", "abc"]).stop_after, None);
    assert_eq!(run(&["--snmp-port", "abc"]).snmp_port, 161);
    assert_eq!(run(&["-t", "abc"]).refresh_time, 2.0);
    let a = run(&["--disable-plugin", " , ,"]);
    assert!(a.disable_plugins.is_empty());
}
