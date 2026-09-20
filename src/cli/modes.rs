//! Startup-time modes and plugin registration (keeps `main.rs` lean).
//!
//! One-shot printers (`--fetch`, `--modules-list`) and the shared
//! `register()` entry point used by every long-running mode.

use crate::cli::args::Args;
use crate::core::config::Config;
use crate::core::logger;
use crate::core::stats::GlancesStats;

const LIGHT_DISABLED: &[&str] = &[
    "percpu", "irq", "sensors", "gpu", "npu", "wifi", "raid", "folders",
    "ports", "connections", "containers", "cloud", "amps", "alert", "mpp",
    "smart", "vms",
];

/// Register plugins honoring --enable-plugin, --disable-plugin, and the
/// --light subset. Single entry point so every mode agrees.
pub fn register(stats: &GlancesStats, args: &Args, config: &Config) {
    // Long-running modes record per-plugin numeric history unless
    // `--disable-history` was passed (upstream default is on).
    stats
        .history_enabled
        .store(!args.disable_history, std::sync::atomic::Ordering::Relaxed);
    let mut disabled: Vec<String> = args.disable_plugins.clone();
    if args.disable_process {
        disabled.extend(
            ["processcount", "processlist", "programlist"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    if args.light {
        disabled.extend(LIGHT_DISABLED.iter().map(|s| s.to_string()));
    }
    crate::plugins::register_filtered(stats, &disabled, &args.enable_plugins);
    // Load `[<plugin>] careful/warning/critical` thresholds into each
    // plugin's limits map — feeds /api/<p>/limits and future alerting.
    stats.apply_limits_config(config);
}

/// `--fetch`: neofetch-style summary printed once and exit (upstream
/// `--stdout-fetch` parity). One refresh tick, then host + load lines.
pub fn print_fetch(refresh_secs: f32, args: &Args, config: &Config) {
    let stats = GlancesStats::new(refresh_secs);
    register(&stats, args, config);
    if let Err(e) = stats.update() {
        logger::warning(&format!("fetch: stats.update() failed: {}", e));
    }
    let snap = stats.snapshot();
    let str_of = |plugin: &str, key: &str| -> String {
        snap.as_object()
            .and_then(|o| o.get(plugin))
            .and_then(|p| p.as_object())
            .and_then(|o| o.get(key))
            .and_then(|v| match v {
                crate::core::value::Value::String(s) => Some(s.clone()),
                _ => v.as_f64().map(|n| format!("{}", n)),
            })
            .unwrap_or_else(|| "-".to_string())
    };
    let num = |plugin: &str, key: &str| -> f64 {
        snap.as_object()
            .and_then(|o| o.get(plugin))
            .and_then(|p| p.as_object())
            .and_then(|o| o.get(key))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    };
    let template = args.fetch_template.clone().unwrap_or_default();
    if !template.is_empty() {
        println!("{}", template);
        return;
    }
    println!("glances-rs {}", env!("CARGO_PKG_VERSION"));
    println!("-------------------");
    println!("Host: {}", str_of("system", "hostname"));
    println!("OS: {} {}", str_of("system", "os_name"), str_of("system", "os_version"));
    println!("Kernel: {}", str_of("system", "kernel"));
    println!("Uptime: {}s", num("uptime", "seconds") as u64);
    println!("CPU: {:.1}% ({} cores)", num("cpu", "total"), num("cpu", "cpucore") as u64);
    println!("Memory: {:.1}%", num("mem", "percent"));
    println!(
        "Load: {:.2} {:.2} {:.2}",
        num("load", "min1"),
        num("load", "min5"),
        num("load", "min15")
    );
    println!("Processes: {}", num("processcount", "total") as u64);
}

/// `--modules-list`: plugin + exporter inventory and exit.
pub fn print_modules() {
    println!("Plugins:");
    for name in crate::plugins::plugin_names() {
        println!("  {}", name);
    }
    println!("Exporters:");
    for name in crate::exports::exporter_names() {
        println!("  {}", name);
    }
}
