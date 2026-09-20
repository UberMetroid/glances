//! GlancesStats — plugin registry + refresh loop.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use super::error::Result;
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
}

impl GlancesStats {
    pub fn new(refresh_time: f32) -> Self {
        Self {
            plugins: RwLock::new(Vec::new()),
            refresh_time,
            history_enabled: std::sync::atomic::AtomicBool::new(true),
        }
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
                        record_history(plugin.as_mut());
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
    /// section. Python Glances does the same: numeric `*_careful|_warning|
    /// _critical` keys become floats, the rest CSV lists.
    pub fn apply_limits_config(&self, cfg: &crate::core::config::Config) {
        let mut guard = self.plugins.write().unwrap_or_else(|e| e.into_inner());
        for p in guard.iter_mut() {
            let Some(section) = cfg.section(p.name()) else { continue };
            let Some(model) = p.model_mut() else { continue };
            for (k, v) in section {
                let lv = match v.parse::<f64>() {
                    Ok(f) => crate::core::plugin::LimitValue::Float(f),
                    Err(_) => crate::core::plugin::LimitValue::List(
                        v.split(',').map(|s| s.trim().to_string()).collect()),
                };
                model.limits.insert(k.clone(), lv);
            }
        }
    }
}

/// Record every top-level numeric field of a plugin's stats into its
/// history ring (mirrors Python Glances' per-stat history). Nested
/// objects/arrays are skipped — element series keep their own rows.
fn record_history(plugin: &mut dyn Plugin) {
    let Some(model) = plugin.model_mut() else { return };
    let nums: Vec<(String, f64)> = match model.stats.as_object() {
        Some(obj) => obj
            .iter()
            .filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n)))
            .collect(),
        None => return,
    };
    for (k, n) in nums {
        model.stats_history.add(&k, n);
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
