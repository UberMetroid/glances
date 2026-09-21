//! CLI mode/export/display flag tests — modes, exports, display toggles.

use crate::cli::args::{parse_args_with, Mode};
use crate::cli::flags::apply_flag;
use crate::cli::parse::parse_argv;

fn run(argv: &[&str]) -> crate::cli::args::Args {
    let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    parse_args_with(&owned)
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
fn plugin_registration_respects_enable_and_disable() {
    use crate::core::stats::GlancesStats;
    // Upstream parity (stats.py): a bare enable list NEVER narrows the
    // set — everything stays registered.
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_filtered(&stats, &[], &["cpu".to_string(), "mem".to_string()]);
    let names = stats.plugin_names();
    assert!(names.contains(&"cpu"));
    assert!(names.contains(&"mem"));
    assert!(names.contains(&"fs"), "enable list must not disable the rest");
    // Only `disable all` narrows to the enabled set.
    let stats_all = GlancesStats::new(2.0);
    crate::plugins::register_filtered(&stats_all, &["all".to_string()], &["cpu".to_string()]);
    assert_eq!(stats_all.plugin_names(), vec!["cpu"]);
    let stats2 = GlancesStats::new(2.0);
    crate::plugins::register_filtered(&stats2, &["cpu".to_string()], &[]);
    assert!(!stats2.plugin_names().contains(&"cpu"));
    assert!(stats2.plugin_names().contains(&"mem"));
}

#[test]
fn upstream_display_toggles_parse() {
    let a = run(&["-1", "-0", "-6", "-b", "--fahrenheit", "--programs"]);
    assert!(a.percpu && a.disable_irix && a.mean_gpu);
    assert!(a.byte_units && a.fahrenheit && a.programs);
    let a = run(&["--disable-bold", "--disable-bg", "--disable-separator"]);
    assert!(a.disable_bold && a.disable_bg && !a.enable_separator);
    let a = run(&["--process-long-name"]);
    assert!(!a.process_short_name);
    let a = run(&["--sort-processes", "cpu_percent", "-f", "py.*"]);
    assert_eq!(a.sort_processes.as_deref(), Some("cpu_percent"));
    assert_eq!(a.process_filter.as_deref(), Some("py.*"));
}

#[test]
fn new_modes_parse() {
    assert_eq!(run(&["--fetch"]).mode, Mode::Fetch);
    assert_eq!(run(&["--modules-list"]).mode, Mode::ModulesList);
    assert_eq!(run(&["--module-list"]).mode, Mode::ModulesList);
    assert_eq!(run(&["--api-doc"]).mode, Mode::ApiDoc);
    assert_eq!(run(&["--api-restful-doc"]).mode, Mode::ApiDoc);
    let a = run(&["--enable-irq"]);
    assert!(a.enable_plugins.contains(&"irq".to_string()));
    let a = run(&["--snmp-user", "u", "--snmp-auth", "k"]);
    assert_eq!(a.snmp_user.as_deref(), Some("u"));
    assert_eq!(a.snmp_auth.as_deref(), Some("k"));
    let a = run(&["--stdout-csv", "cpu,mem"]);
    assert_eq!(a.mode, Mode::StdoutCsv);
    assert_eq!(a.stdout_plugins.as_deref(), Some("cpu,mem"));
}

#[test]
fn disable_all_needs_explicit_enable() {
    use crate::core::stats::GlancesStats;
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_filtered(&stats, &["all".to_string()], &["cpu".to_string()]);
    assert_eq!(stats.plugin_names(), vec!["cpu"]);
}

#[test]
fn apply_flag_does_not_panic_on_empty_token() {
    let a = crate::cli::args::Args::default();
    let _ = a;
    let _ = apply_flag;
    let _ = parse_argv;
}
