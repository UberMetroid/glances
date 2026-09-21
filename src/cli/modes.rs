//! Startup-time modes and plugin registration (keeps `main.rs` lean).
//!
//! One-shot printers (`--fetch`, `--modules-list`) and the shared
//! `register()` entry point used by every long-running mode.

use crate::cli::args::Args;
use crate::core::config::Config;
use crate::core::logger;
use crate::core::stats::GlancesStats;

/// Upstream curses left sidebar (`glances_curses.py _left_sidebar`);
/// hidden by `-2/--disable-left-sidebar`.
const LEFT_SIDEBAR: &[&str] = &[
    "network", "ports", "wifi", "connections", "diskio", "fs", "irq",
    "folders", "raid", "smart", "sensors", "now",
];

/// Register plugins honoring --enable-plugin, --disable-plugin, and the
/// -2/-3/-4/-5/--light display subsets. Single entry point so every
/// mode agrees. Each flag keeps its own upstream meaning instead of
/// collapsing into one light mode (`main.py init_ui_mode`).
pub fn register(stats: &GlancesStats, args: &Args, config: &Config) {
    // Long-running modes record per-plugin numeric history unless
    // `--disable-history` was passed (upstream default is on).
    stats
        .history_enabled
        .store(!args.disable_history, std::sync::atomic::Ordering::Relaxed);
    let mut disabled: Vec<String> = args.disable_plugins.clone();
    let mut enabled: Vec<String> = args.enable_plugins.clone();
    if args.disable_process {
        disabled.extend(
            ["processcount", "processlist", "programlist"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    if args.disable_left_sidebar || args.light {
        disabled.extend(LEFT_SIDEBAR.iter().map(|s| s.to_string()));
    }
    if args.disable_quicklook {
        disabled.push("quicklook".to_string());
    }
    if args.full_quicklook {
        // Upstream keeps quicklook+load, drops the gauges between them.
        disabled.extend(["cpu", "mem", "memswap"].iter().map(|s| s.to_string()));
        for p in ["quicklook", "load"] {
            if !enabled.iter().any(|e| e == p) {
                enabled.push(p.to_string());
            }
        }
    }
    if args.disable_top {
        disabled.extend(
            ["quicklook", "cpu", "mem", "memswap", "load"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    if args.light {
        // Upstream `--light`: left sidebar off plus the heavy/loud
        // plugins (`main.py init_ui_mode` manage-light block).
        disabled.extend(
            [
                "processcount", "processlist", "programlist", "alert", "amps",
                "containers", "vms",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
    }
    crate::plugins::register_filtered(stats, &disabled, &enabled);
    // Alert-command posture: `--disable-config-exec` forces
    // single-process action execution (upstream GlancesActions parity).
    stats.set_actions_allow_operators(!args.disable_config_exec);
    // Display process filter (`-f/--process-filter` parity): only
    // matching processes are published by processlist.
    // `-0/--disable-irix`: per-process CPU% divided by core count.
    if args.process_filter.is_some() || args.disable_irix {
        let mut guard = stats.plugins.write().unwrap_or_else(|e| e.into_inner());
        for p in guard.iter_mut() {
            p.set_process_filter(args.process_filter.as_deref());
            p.set_irix_divide(args.disable_irix);
        }
    }
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

/// `--modules-list`: plugin inventory and exit.
pub fn print_modules() {
    println!("Plugins:");
    for name in crate::plugins::plugin_names() {
        println!("  {}", name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::Args;
    use crate::core::config::Config;

    fn registered(args: &Args) -> Vec<&'static str> {
        let stats = GlancesStats::new(2.0);
        register(&stats, args, &Config::empty());
        stats.plugin_names()
    }

    fn with(f: impl FnOnce(&mut Args)) -> Args {
        let mut args = Args::default();
        f(&mut args);
        args
    }

    #[test]
    fn left_sidebar_flag_hides_only_the_sidebar() {
        let names = registered(&with(|a| a.disable_left_sidebar = true));
        for p in [
            "network", "ports", "wifi", "connections", "diskio", "fs",
            "irq", "folders", "raid", "smart", "sensors", "now",
        ] {
            assert!(!names.contains(&p), "{p} must be hidden by -2");
        }
        for p in ["cpu", "mem", "load", "quicklook", "processlist", "alert"] {
            assert!(names.contains(&p), "{p} must survive -2");
        }
    }

    #[test]
    fn quicklook_flag_hides_only_quicklook() {
        let names = registered(&with(|a| a.disable_quicklook = true));
        assert!(!names.contains(&"quicklook"));
        for p in ["cpu", "mem", "load", "network", "fs"] {
            assert!(names.contains(&p), "{p} must survive -3");
        }
    }

    #[test]
    fn full_quicklook_keeps_quicklook_and_load() {
        let names = registered(&with(|a| a.full_quicklook = true));
        for p in ["cpu", "mem", "memswap"] {
            assert!(!names.contains(&p), "{p} must be hidden by -4");
        }
        for p in ["quicklook", "load"] {
            assert!(names.contains(&p), "{p} must survive -4");
        }
    }

    #[test]
    fn disable_top_hides_exactly_the_startup_set() {
        let names = registered(&with(|a| a.disable_top = true));
        for p in ["quicklook", "cpu", "mem", "memswap", "load"] {
            assert!(!names.contains(&p), "{p} must be hidden by -5");
        }
        for p in ["network", "fs", "processlist"] {
            assert!(names.contains(&p), "{p} must survive -5");
        }
    }

    #[test]
    fn light_mode_matches_upstream_manage_light() {
        let names = registered(&with(|a| a.light = true));
        for p in [
            "network", "ports", "wifi", "connections", "diskio", "fs",
            "irq", "folders", "raid", "smart", "sensors", "now",
            "processcount", "processlist", "programlist", "alert", "amps",
            "containers", "vms",
        ] {
            assert!(!names.contains(&p), "{p} must be hidden by --light");
        }
        for p in ["cpu", "mem", "load", "quicklook"] {
            assert!(names.contains(&p), "{p} must survive --light");
        }
    }
}
