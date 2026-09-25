//! Mode, export, display-toggle, and registration-rule flag tests.

use crate::cli::args::{parse_args_with, Mode};

fn run(argv: &[&str]) -> crate::cli::args::Args {
    parse_args_with(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn mcp_path_defaults_and_sets() {
    assert_eq!(crate::cli::args::Args::default().mcp_path, "/mcp");
    assert_eq!(run(&["--mcp-path", "/api/mcp"]).mcp_path, "/api/mcp");
}

#[test]
fn stdout_modes_and_specs() {
    assert_eq!(run(&["--stdout-csv"]).mode, Mode::StdoutCsv);
    assert_eq!(run(&["--stdout-json"]).mode, Mode::StdoutJson);
    let a = run(&["--stdout", "cpu.total,mem.percent"]);
    assert_eq!(a.mode, Mode::StdoutPath);
    assert_eq!(a.stdout_spec.as_deref(), Some("cpu.total,mem.percent"));
}

#[test]
fn ports_and_plugin_lists() {
    let a = run(&["--web-port", "8888"]);
    assert_eq!(a.web_port, 8888);
    assert_eq!(a.mode, Mode::Standalone);
    let a = run(&["--disable-plugin", "fs,diskio", "--enable-plugin", "cpu"]);
    assert_eq!(a.disable_plugins, vec!["fs", "diskio"]);
    assert_eq!(a.enable_plugins, vec!["cpu"]);
}

#[test]
fn stray_words_change_nothing() {
    let a = run(&["stray-argument"]);
    assert_eq!(a.mode, Mode::Standalone);
    assert_eq!(a.stdout_spec, None);
}

#[test]
fn refresh_rejects_garbage_and_non_positive() {
    for bad in ["nan", "inf", "-inf", "0", "-3", "abc"] {
        assert_eq!(run(&["-t", bad]).refresh_time, 2.0, "-t {bad} must be rejected");
    }
}

#[test]
fn enable_lists_never_narrow() {
    use crate::core::stats::GlancesStats;
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_filtered(&stats, &[], &["cpu".to_string(), "mem".to_string()]);
    let names = stats.plugin_names();
    assert!(names.contains(&"cpu") && names.contains(&"mem"));
    assert!(names.contains(&"fs"), "a bare enable list must not narrow");
    let stats2 = GlancesStats::new(2.0);
    crate::plugins::register_filtered(&stats2, &["cpu".to_string()], &[]);
    assert!(!stats2.plugin_names().contains(&"cpu"));
    assert!(stats2.plugin_names().contains(&"mem"));
}

#[test]
fn narrow_needs_disable_all_plus_enable() {
    use crate::core::stats::GlancesStats;
    for stats in [GlancesStats::new(2.0), GlancesStats::new(2.0)] {
        crate::plugins::register_filtered(&stats, &["all".to_string()], &["cpu".to_string()]);
        assert_eq!(stats.plugin_names(), vec!["cpu"]);
    }
}

#[test]
fn display_toggle_bundle() {
    let a = run(&["-1", "-0", "-6", "-b", "--fahrenheit", "--programs"]);
    assert!(a.percpu && a.disable_irix && a.mean_gpu);
    assert!(a.byte_units && a.fahrenheit && a.programs);
    let a = run(&["--disable-bold", "--disable-bg", "--disable-separator"]);
    assert!(a.disable_bold && a.disable_bg && !a.enable_separator);
    assert!(!run(&["--process-long-name"]).process_short_name);
    let a = run(&["--sort-processes", "cpu_percent", "-f", "py.*"]);
    assert_eq!(a.sort_processes.as_deref(), Some("cpu_percent"));
    assert_eq!(a.process_filter.as_deref(), Some("py.*"));
}

#[test]
fn one_shot_modes_and_valued_extras() {
    assert_eq!(run(&["--fetch"]).mode, Mode::Fetch);
    assert_eq!(run(&["--modules-list"]).mode, Mode::ModulesList);
    assert_eq!(run(&["--module-list"]).mode, Mode::ModulesList);
    assert_eq!(run(&["--api-doc"]).mode, Mode::ApiDoc);
    assert_eq!(run(&["--api-restful-doc"]).mode, Mode::ApiDoc);
    assert!(run(&["--enable-irq"]).enable_plugins.contains(&"irq".to_string()));
    let a = run(&["--snmp-user", "u", "--snmp-auth", "k"]);
    assert_eq!(a.snmp_user.as_deref(), Some("u"));
    assert_eq!(a.snmp_auth.as_deref(), Some("k"));
    let a = run(&["--stdout-csv", "cpu,mem"]);
    assert_eq!(a.mode, Mode::StdoutCsv);
    assert_eq!(a.stdout_plugins.as_deref(), Some("cpu,mem"));
}

#[test]
fn flag_entry_points_link() {
    let _ = crate::cli::flags::apply_flag;
    let _ = crate::cli::parse::parse_argv;
}
