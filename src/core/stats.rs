//! GlancesStats — plugin registry + refresh loop.

use std::collections::BTreeMap;
use std::sync::RwLock;

use super::actions::GlancesActions;
use super::error::Result;
use super::events::EventLog;
use super::plugin::Plugin;
use super::value::Value;

/// Plugin registry (`RwLock` so readers snapshot while the loop mutates).
pub struct GlancesStats {
    pub plugins: RwLock<Vec<Box<dyn Plugin>>>,
    pub refresh_time: f32,
    /// Per-plugin numeric history on each tick (upstream default on;
    /// `--disable-history` flips it off). Atomic for `Arc` sharing.
    pub history_enabled: std::sync::atomic::AtomicBool,
    /// Global alert event log (upstream `glances_events` parity).
    pub events: std::sync::Mutex<EventLog>,
    /// Alert-command runner (upstream `GlancesActions` parity).
    pub actions: std::sync::Mutex<GlancesActions>,
    /// PID with extended stats pinned (upstream
    /// `glances_processes.extended_process` parity).
    pub extended_process: std::sync::Mutex<Option<u32>>,
    /// Unix seconds of the last served web request (idle detection).
    /// Stamped by the server; read by the refresh loop. Starts at
    /// construction time so a fresh server ticks fast immediately.
    pub last_served: std::sync::atomic::AtomicU64,
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
            extended_process: std::sync::Mutex::new(None),
            last_served: std::sync::atomic::AtomicU64::new(super::idle::unix_now()),
        }
    }

    /// Record that the web server just served a request — proof
    /// somebody is watching, so the refresh loop stays fast.
    pub fn mark_served(&self) {
        self.last_served.store(super::idle::unix_now(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Unix seconds of the last served request.
    pub fn last_served_secs(&self) -> u64 {
        self.last_served.load(std::sync::atomic::Ordering::Relaxed)
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

    /// Drive one refresh tick. Plugin panics are caught via
    /// `catch_unwind` so one bad plugin cannot kill the loop.
    pub fn update(&self) -> Result<()> { self.update_inner(None) }

    /// SNMP client-mode tick (upstream `GlancesStatsClientSNMP.update`
    /// parity): plugins poll the agent; history/views/actions tail runs
    /// unchanged. Unsupported plugins log and keep stale stats.
    pub fn update_snmp(&self, ctx: &super::snmp::SnmpCtx) -> Result<()> {
        self.update_inner(Some(ctx))
    }

    fn update_inner(&self, snmp: Option<&super::snmp::SnmpCtx>) -> Result<()> {
        let mut guard = self.plugins.write().unwrap_or_else(|e| e.into_inner());
        for plugin in guard.iter_mut() {
            if !plugin.is_enabled() { continue; }
            let name = plugin.name();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match snmp {
                    Some(ctx) => plugin.update_snmp(ctx),
                    None => plugin.update(),
                }
            }));
            match result {
                Ok(Ok(())) => {
                    if self.history_enabled.load(std::sync::atomic::Ordering::Relaxed) {
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
                    // Refresh alert decorations, then fire `*_action`
                    // commands for live triggers.
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
                    // Unsupported SNMP input is routine (most plugins
                    // have no MIB table); anything else is a warning.
                    match e {
                        super::error::GlancesError::Unsupported(_) => {
                            super::logger::debug(&format!("plugin {}: {}", name, e));
                        }
                        _ => super::logger::warning(&format!(
                            "plugin {} update returned error: {}", name, e
                        )),
                    }
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
        // Quicklook reads sibling stats after all updates complete.
        aggregate_quicklook(&mut guard);
        Ok(())
    }


    /// Populate each plugin's `limits` map from its `[<plugin>]` config
    /// section plus upstream built-in defaults (`set_default` parity).
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
                // Keys are stored prefixed (`[mem] careful=60` →
                // `mem_careful`), which is what `get_limit` looks up.
                model.limits.insert(format!("{}_{}", plugin_name, k), lv);
            }
        }
    }
}

impl GlancesStats {
    /// Full snapshot: plugin name -> stats value. This is the value the
    /// web API serializes.
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

