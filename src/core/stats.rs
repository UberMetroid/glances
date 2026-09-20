//! GlancesStats — plugin registry + refresh loop.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use super::actions::GlancesActions;
use super::error::Result;
use super::events::EventLog;
use super::plugin::Plugin;
use super::value::Value;

/// Registry of all loaded plugins, keyed by plugin name (directory name).
///
/// Wrapped in `RwLock` so HTTP/MCP/export readers can snapshot concurrently
/// while the refresh loop is mutating.
pub struct GlancesStats {
    pub plugins: RwLock<Vec<Box<dyn Plugin>>>,
    pub refresh_time: f32,
    /// Record per-plugin numeric history on each tick (upstream default).
    /// `--disable-history` flips this off; the `/history` endpoint and
    /// sparklines read what was recorded. Atomic so startup code can flip
    /// it through a shared reference (including under `Arc`).
    pub history_enabled: std::sync::atomic::AtomicBool,
    /// Global alert event log (upstream `glances_events` parity).
    /// Populated by `update_views` when a `*_log` threshold fires;
    /// consumed by the alert plugin and `/api/4/events` surface.
    pub events: std::sync::Mutex<EventLog>,
    /// Alert-command runner (upstream `GlancesActions` parity).
    /// Fires `*_action` commands for CAREFUL/WARNING/CRITICAL triggers.
    pub actions: std::sync::Mutex<GlancesActions>,
}

impl GlancesStats {
    pub fn new(refresh_time: f32) -> Self {
        let rt = refresh_time;
        Self {
            plugins: RwLock::new(Vec::new()),
            refresh_time,
            history_enabled: std::sync::atomic::AtomicBool::new(true),
            events: std::sync::Mutex::new(EventLog::default()),
            actions: std::sync::Mutex::new(GlancesActions::new(rt, true)),
        }
    }

    /// `--disable-config-exec` parity for alert commands.
    pub fn set_actions_allow_operators(&self, allow: bool) {
        if let Ok(mut a) = self.actions.lock() { a.allow_operators = allow; }
    }

    /// Register a plugin. Order is preserved.
    pub fn register(&self, plugin: Box<dyn Plugin>) {
        let mut guard = self.plugins.write().unwrap_or_else(|e| e.into_inner());
        guard.push(plugin);
    }

    /// Names of all registered plugins in order.
    pub fn plugin_names(&self) -> Vec<&'static str> {
        let guard = self.plugins.read().unwrap_or_else(|e| e.into_inner());
        guard.iter().map(|p| p.name()).collect()
    }

    /// Plugin name → `get_key` element field, for exporters naming
    /// per-element series (`network.eth0`, `fs./`, …).
    pub fn plugin_keys(&self) -> std::collections::HashMap<String, &'static str> {
        let guard = self.plugins.read().unwrap_or_else(|e| e.into_inner());
        guard.iter()
            .filter_map(|p| p.get_key().map(|k| (p.name().to_string(), k)))
            .collect()
    }

    /// Drive one refresh tick. Calls `update()` on each enabled plugin.
    /// Plugin panics are caught via `catch_unwind` so one bad plugin
    /// cannot kill the loop (matches the plan §4.3 recovery semantics).
    pub fn update(&self) -> Result<()> {
        let mut guard = self.plugins.write().unwrap_or_else(|e| e.into_inner());
        for plugin in guard.iter_mut() {
            if !plugin.is_enabled() { continue; }
            let name = plugin.name();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugin.update()
            }));
            match result {
                Ok(Ok(())) => {
                    if self.history_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                        // Curated per-plugin series (upstream
                        // `update_stats_history` parity).
                        let hist = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            plugin.update_stats_history();
                        }));
                        if hist.is_err() {
                            super::logger::error(&format!(
                                "plugin {} panicked during update_stats_history",
                                name
                            ));
                        }
                    }
                    // Upstream `update_plugin` parity: refresh alert
                    // decorations right after the stats update, then
                    // fire `*_action` commands for live triggers.
                    if let Ok(mut acts) = self.actions.lock() {
                        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            super::stats_actions::run_plugin_actions(plugin.as_mut(), &mut acts);
                        }));
                        if ran.is_err() {
                            super::logger::error(&format!(
                                "plugin {} panicked during run_plugin_actions",
                                name
                            ));
                        }
                    }
                    if let Ok(mut ev) = self.events.lock() {
                        let views = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            plugin.update_views(&mut ev);
                        }));
                        if views.is_err() {
                            super::logger::error(&format!(
                                "plugin {} panicked during update_views; views left stale",
                                name
                            ));
                        }
                    }
                }
                Ok(Err(e)) => {
                    super::logger::warning(&format!(
                        "plugin {} update returned error: {}", name, e
                    ));
                }
                Err(_) => {
                    super::logger::error(&format!(
                        "plugin {} panicked during update; leaving previous stats in place",
                        name
                    ));
                    // Do not propagate panic; previous stats remain valid.
                }
            }
        }
        // Cross-plugin consumers (quicklook) read sibling stats after all
        // individual updates complete — same ordering as Python Glances'
        // stats aggregation pass.
        aggregate_quicklook(&mut guard);
        Ok(())
    }


    /// Populate each plugin's `limits` map from its `[<plugin>]` config
    /// section, then fill upstream built-in careful/warning/critical
    /// defaults for missing keys (`set_default` parity — user config
    /// always wins). Python Glances does the same: numeric
    /// `*_careful|_warning|_critical` keys become floats, the rest CSV
    /// lists.
    pub fn apply_limits_config(&self, cfg: &crate::core::config::Config) {
        let ncpu = std::thread::available_parallelism().map(|n| n.get() as u64).unwrap_or(1);
        // Upstream `load_limits` parity: `[global] history_size` lands
        // in every plugin's limits (default 28800).
        let history_size = cfg
            .section("global")
            .and_then(|g| g.get("history_size"))
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(28800.0);
        let mut guard = self.plugins.write().unwrap_or_else(|e| e.into_inner());
        for p in guard.iter_mut() {
            let entries = crate::core::alerts::default_limit_entries(p.name());
            if let Some(model) = p.model_mut() {
                model.apply_default_limits(&entries, ncpu);
                model.limits.insert(
                    "history_size".into(),
                    crate::core::alerts::LimitValue::Float(history_size),
                );
            }
            let plugin_name = p.name();
            let Some(section) = cfg.section(plugin_name) else { continue };
            let Some(model) = p.model_mut() else { continue };
            for (k, v) in section {
                let lv = match v.parse::<f64>() {
                    Ok(f) => crate::core::alerts::LimitValue::Float(f),
                    Err(_) => crate::core::alerts::LimitValue::List(
                        v.split(',').map(|s| s.trim().to_string()).collect()),
                };
                // Upstream `load_limits` parity: every key is stored
                // prefixed with the plugin name (`[mem] careful=60` →
                // `mem_careful`), which is what `get_limit` looks up.
                model.limits.insert(format!("{}_{}", plugin_name, k), lv);
            }
        }
    }
}

impl GlancesStats {
    /// Full snapshot: plugin name -> stats value. This is the value the
    /// web API, XML-RPC `getAll`, and exporters serialize.
    pub fn snapshot(&self) -> Value {
        let mut map = BTreeMap::new();
        let guard = self.plugins.read().unwrap_or_else(|e| e.into_inner());
        for p in guard.iter() {
            map.insert(p.name().to_string(), p.stats().clone());
        }
        Value::Object(map)
    }
}

/// Post-update aggregation: fill the quicklook plugin's summary fields
/// from the just-updated cpu/mem/memswap/load values. Runs inside the
/// same write lock so readers never see a half-filled quicklook.
fn aggregate_quicklook(plugins: &mut [Box<dyn Plugin>]) {
    let num = |name: &str, key: &str| -> Option<f64> {
        plugins.iter().find(|p| p.name() == name)
            .and_then(|p| p.stats().as_object())
            .and_then(|o| o.get(key))
            .and_then(Value::as_f64)
    };
    let cpu_name = plugins.iter().find(|p| p.name() == "cpu")
        .and_then(|p| p.stats().as_object().and_then(|o| o.get("cpu_name")).cloned())
        .unwrap_or(Value::Null);
    let cpu = num("cpu", "total").unwrap_or(0.0);
    let mem = num("mem", "percent").unwrap_or(0.0);
    let swap = num("memswap", "percent").unwrap_or(0.0);
    // Python quicklook: load1 as a percentage of available cores.
    let min1 = num("load", "min1").unwrap_or(0.0);
    let cores = num("load", "cpucore").unwrap_or(0.0);
    let load = if cores > 0.0 { min1 / cores * 100.0 } else { 0.0 };

    for p in plugins.iter_mut() {
        if p.name() != "quicklook" { continue; }
        if let Some(obj) = p.stats_mut().as_object_mut() {
            obj.insert("cpu".into(), Value::Float(cpu));
            obj.insert("mem".into(), Value::Float(mem));
            obj.insert("swap".into(), Value::Float(swap));
            obj.insert("load".into(), Value::Float(load));
            obj.insert("cpu_name".into(), cpu_name.clone());
        }
    }
}

/// Spawn the background refresh loop used by modes without their own
/// update driver (web server, XML-RPC server): every `refresh_secs`
/// update all plugins, then fan out to `--export` targets. No-op for
/// non-finite/non-positive intervals.
pub fn spawn_refresh_loop(stats: Arc<GlancesStats>, refresh_secs: f32, args: crate::cli::args::Args) {
    if !(refresh_secs.is_finite() && refresh_secs > 0.0) {
        return;
    }
    std::thread::spawn(move || {
        loop {
            if let Err(e) = stats.update() {
                super::logger::warning(&format!("refresh: stats.update() failed: {}", e));
            }
            if !args.export_targets.is_empty() {
                let keys = stats.plugin_keys();
                crate::exports::write_targets(&stats.snapshot(), &args, &keys);
            }
            std::thread::sleep(std::time::Duration::from_secs_f32(refresh_secs));
        }
    });
}
