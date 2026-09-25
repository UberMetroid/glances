//! Mode support: plugin-set selection and one-shot printers.
//!
//! `register` is the single entry every long-running mode shares, so
//! flag effects agree everywhere. The printers serve `--fetch` and
//! `--modules-list`.

use crate::cli::args::Args;
use crate::core::config::Config;
use crate::core::logger;
use crate::core::stats::GlancesStats;

/// The left-sidebar plugin set (`-2` hides exactly these).
const LEFT_SIDEBAR: &[&str] = &[
    "network", "ports", "wifi", "connections", "diskio", "fs", "irq",
    "folders", "raid", "smart", "sensors", "now",
];

/// Top summary set (`-5` hides exactly these).
const TOP_SET: &[&str] = &["quicklook", "cpu", "mem", "memswap", "load"];

/// Heavy set (`--light` hides the sidebar plus these).
const LIGHT_EXTRA: &[&str] = &[
    "processcount", "processlist", "programlist", "alert", "amps",
    "containers", "vms",
];

const PROCESS_FAMILY: &[&str] = &["processcount", "processlist", "programlist"];

/// Register the plugin set for the requested display subset, then set
/// history posture, action posture, the process filter, and both
/// config applications. Each subset flag keeps its own meaning.
pub fn register(stats: &GlancesStats, args: &Args, config: &Config) {
    stats.history_enabled.store(!args.disable_history, std::sync::atomic::Ordering::Relaxed);
    let mut disabled: Vec<String> = args.disable_plugins.clone();
    let mut enabled: Vec<String> = args.enable_plugins.clone();
    let mut off = |names: &[&str]| disabled.extend(names.iter().map(|s| s.to_string()));
    let mut on = |name: &str| {
        if !enabled.iter().any(|e| e == name) {
            enabled.push(name.to_string());
        }
    };
    if args.disable_process {
        off(PROCESS_FAMILY);
    }
    if args.disable_left_sidebar || args.light {
        off(LEFT_SIDEBAR);
    }
    if args.disable_quicklook {
        off(&["quicklook"]);
    }
    if args.full_quicklook {
        // Keeps quicklook+load, drops the gauges between them.
        off(&["cpu", "mem", "memswap"]);
        on("quicklook");
        on("load");
    }
    if args.disable_top {
        off(TOP_SET);
    }
    if args.light {
        off(LIGHT_EXTRA);
    }
    crate::plugins::register_filtered(stats, &disabled, &enabled);
    stats.set_actions_allow_operators(!args.disable_config_exec);
    if args.process_filter.is_some() || args.disable_irix {
        let mut guard = stats.plugins.write().unwrap_or_else(|e| e.into_inner());
        for p in guard.iter_mut() {
            p.set_process_filter(args.process_filter.as_deref());
            p.set_irix_divide(args.disable_irix);
        }
    }
    stats.apply_limits_config(config);
    stats.apply_plugin_config(config);
}

/// `--fetch`: one tick, then a fixed host summary (or the template
/// verbatim when one is set). Missing readings show `-`/0.
pub fn print_fetch(refresh_secs: f32, args: &Args, config: &Config) {
    let stats = GlancesStats::new(refresh_secs);
    register(&stats, args, config);
    if let Err(e) = stats.update() {
        logger::warning(&format!("fetch: stats.update() failed: {e}"));
    }
    if let Some(t) = args.fetch_template.clone().filter(|t| !t.is_empty()) {
        println!("{t}");
        return;
    }
    let snap = stats.snapshot();
    let num = |plugin: &str, key: &str| -> f64 {
        snap.as_object()
            .and_then(|o| o.get(plugin))
            .and_then(|p| p.as_object())
            .and_then(|o| o.get(key))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    };
    let word = |plugin: &str, key: &str| -> String {
        snap.as_object()
            .and_then(|o| o.get(plugin))
            .and_then(|p| p.as_object())
            .and_then(|o| o.get(key))
            .and_then(|v| match v {
                crate::core::value::Value::String(s) => Some(s.clone()),
                _ => v.as_f64().map(|n| format!("{n}")),
            })
            .unwrap_or_else(|| "-".to_string())
    };
    println!("glances-rs {}", env!("CARGO_PKG_VERSION"));
    println!("-------------------");
    println!("Host: {}", word("system", "hostname"));
    println!("OS: {} {}", word("system", "os_name"), word("system", "os_version"));
    println!("Kernel: {}", word("system", "kernel"));
    println!("Uptime: {}s", num("uptime", "seconds") as u64);
    println!("CPU: {:.1}% ({} cores)", num("cpu", "total"), num("cpu", "cpucore") as u64);
    println!("Memory: {:.1}%", num("mem", "percent"));
    println!("Load: {:.2} {:.2} {:.2}", num("load", "min1"), num("load", "min5"), num("load", "min15"));
    println!("Processes: {}", num("processcount", "total") as u64);
}

/// `--modules-list`: the plugin inventory, one per line.
pub fn print_modules() {
    println!("Plugins:");
    for name in crate::plugins::plugin_names() {
        println!("  {name}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::Args;
    use crate::core::config::Config;

    fn registered_with(f: impl FnOnce(&mut Args)) -> Vec<&'static str> {
        let mut args = Args::default();
        f(&mut args);
        let stats = GlancesStats::new(2.0);
        register(&stats, &args, &Config::empty());
        stats.plugin_names()
    }

    #[test]
    fn sidebar_subset_hides_exactly_its_set() {
        let names = registered_with(|a| a.disable_left_sidebar = true);
        for p in LEFT_SIDEBAR {
            assert!(!names.contains(p), "{p} must be hidden by -2");
        }
        for p in ["cpu", "mem", "load", "quicklook", "processlist", "alert"] {
            assert!(names.contains(&p), "{p} must survive -2");
        }
    }

    #[test]
    fn quicklook_flag_is_surgical() {
        let names = registered_with(|a| a.disable_quicklook = true);
        assert!(!names.contains(&"quicklook"));
        for p in ["cpu", "mem", "load", "network", "fs"] {
            assert!(names.contains(&p), "{p} must survive -3");
        }
    }

    #[test]
    fn full_quicklook_trades_gauges_for_load() {
        let names = registered_with(|a| a.full_quicklook = true);
        for p in ["cpu", "mem", "memswap"] {
            assert!(!names.contains(&p), "{p} must be hidden by -4");
        }
        for p in ["quicklook", "load"] {
            assert!(names.contains(&p), "{p} must survive -4");
        }
    }

    #[test]
    fn top_flag_hides_the_startup_set() {
        let names = registered_with(|a| a.disable_top = true);
        for p in TOP_SET {
            assert!(!names.contains(p), "{p} must be hidden by -5");
        }
        for p in ["network", "fs", "processlist"] {
            assert!(names.contains(&p), "{p} must survive -5");
        }
    }

    #[test]
    fn light_mode_hides_sidebar_and_heavy_sets() {
        let names = registered_with(|a| a.light = true);
        for p in LEFT_SIDEBAR.iter().chain(LIGHT_EXTRA.iter()) {
            assert!(!names.contains(p), "{p} must be hidden by --light");
        }
        for p in ["cpu", "mem", "load", "quicklook"] {
            assert!(names.contains(&p), "{p} must survive --light");
        }
    }
}
