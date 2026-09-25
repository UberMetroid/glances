//! Stats hub: plugin registry, refresh ticks, snapshots.
//!
//! Plugins register once, then every tick refreshes each one (guarded
//! so a failing plugin never stops the loop), and readers snapshot
//! the whole tree under a read lock.

use std::collections::BTreeMap;
use std::sync::RwLock;

use super::actions::GlancesActions;
use super::error::Result;
use super::events::EventLog;
use super::plugin::Plugin;
use super::value::Value;

pub struct GlancesStats {
    pub plugins: RwLock<Vec<Box<dyn Plugin>>>,
    pub refresh_time: f32,
    /// Record per-plugin numeric history each tick (`--disable-history`
    /// turns it off).
    pub history_enabled: std::sync::atomic::AtomicBool,
    /// Shared alert event log.
    pub events: std::sync::Mutex<EventLog>,
    /// Alert-command runner.
    pub actions: std::sync::Mutex<GlancesActions>,
    /// PID with pinned extended process stats.
    pub extended_process: std::sync::Mutex<Option<u32>>,
    /// Unix seconds of the last served web request (idle detection).
    /// Starts at construction so a fresh server ticks fast immediately.
    pub last_served: std::sync::atomic::AtomicU64,
}

impl GlancesStats {
    pub fn new(refresh_time: f32) -> Self {
        Self {
            plugins: RwLock::new(Vec::new()),
            refresh_time,
            history_enabled: std::sync::atomic::AtomicBool::new(true),
            events: std::sync::Mutex::new(EventLog::default()),
            actions: std::sync::Mutex::new(GlancesActions::new(refresh_time, true)),
            extended_process: std::sync::Mutex::new(None),
            last_served: std::sync::atomic::AtomicU64::new(super::idle::unix_now()),
        }
    }

    /// Note that the web server just served a request — someone is
    /// watching, so the refresh loop stays fast.
    pub fn mark_served(&self) {
        self.last_served.store(super::idle::unix_now(), std::sync::atomic::Ordering::Relaxed);
    }

    pub fn last_served_secs(&self) -> u64 {
        self.last_served.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Whether alert commands may use shell operators.
    pub fn set_actions_allow_operators(&self, allow: bool) {
        if let Ok(mut a) = self.actions.lock() { a.allow_operators = allow; }
    }

    /// Register a plugin (order preserved).
    pub fn register(&self, plugin: Box<dyn Plugin>) {
        self.plugins.write().unwrap_or_else(|e| e.into_inner()).push(plugin);
    }

    /// Registered plugin names in order.
    pub fn plugin_names(&self) -> Vec<&'static str> {
        self.plugins.read().unwrap_or_else(|e| e.into_inner()).iter().map(|p| p.name()).collect()
    }

    /// One local refresh tick over every enabled plugin.
    pub fn update(&self) -> Result<()> { self.tick(None) }

    /// One SNMP client-mode tick: plugins poll the agent instead of the
    /// local machine; history, actions, and views run unchanged.
    pub fn update_snmp(&self, ctx: &super::snmp::SnmpCtx) -> Result<()> {
        self.tick(Some(ctx))
    }

    fn tick(&self, snmp: Option<&super::snmp::SnmpCtx>) -> Result<()> {
        let mut guard = self.plugins.write().unwrap_or_else(|e| e.into_inner());
        for plugin in guard.iter_mut() {
            if !plugin.is_enabled() {
                continue;
            }
            let name = plugin.name();
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match snmp {
                    Some(ctx) => plugin.update_snmp(ctx),
                    None => plugin.update(),
                }
            }));
            match outcome {
                Ok(Ok(())) => {
                    self.after_update(plugin.as_mut(), name);
                }
                Ok(Err(e)) => {
                    // Unsupported SNMP input is routine (most plugins
                    // have no MIB table); anything else is a warning.
                    match e {
                        super::error::GlancesError::Unsupported(_) => {
                            super::logger::debug(&format!("plugin {name}: {e}"));
                        }
                        _ => super::logger::warning(&format!(
                            "plugin {name} update returned error: {e}"
                        )),
                    }
                }
                Err(_) => {
                    super::logger::error(&format!(
                        "plugin {name} panicked during update; leaving previous stats in place"
                    ));
                }
            }
        }
        // Quicklook summarizes siblings, so it runs after every update,
        // still under the write lock (no half-filled reads).
        fill_quicklook(&mut guard);
        Ok(())
    }

    /// Post-update tail for one plugin: history, alert commands, then
    /// decoration rebuild — each step independently guarded.
    fn after_update(&self, plugin: &mut dyn Plugin, name: &str) {
        if self.history_enabled.load(std::sync::atomic::Ordering::Relaxed)
            && guarded(|| plugin.update_stats_history()).is_err() {
                super::logger::error(&format!(
                    "plugin {name} panicked during update_stats_history"
                ));
            }
        if let Ok(mut acts) = self.actions.lock()
            && guarded(|| super::stats_actions::run_plugin_actions(plugin, &mut acts)).is_err() {
                super::logger::error(&format!(
                    "plugin {name} panicked during run_plugin_actions"
                ));
            }
        if let Ok(mut ev) = self.events.lock()
            && guarded(|| plugin.update_views(&mut ev)).is_err() {
                super::logger::error(&format!(
                    "plugin {name} panicked during update_views; views left stale"
                ));
            }
    }

    /// Fill each plugin's limits: built-in defaults, the global
    /// `history_size` (default 28800), then its `[<plugin>]` config
    /// section stored plugin-prefixed (`[mem] careful=60` lands as
    /// `mem_careful`, which is what limit lookups read). Values parse as
    /// floats, else comma-split lists.
    pub fn apply_limits_config(&self, cfg: &crate::core::config::Config) {
        let ncpu = std::thread::available_parallelism().map(|n| n.get() as u64).unwrap_or(1);
        let history_size = cfg
            .section("global")
            .and_then(|g| g.get("history_size"))
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(28800.0);
        let mut guard = self.plugins.write().unwrap_or_else(|e| e.into_inner());
        for p in guard.iter_mut() {
            let plugin_name = p.name();
            if let Some(model) = p.model_mut() {
                model.apply_default_limits(&crate::core::alerts::default_limit_entries(plugin_name), ncpu);
                model.limits.insert("history_size".into(), crate::core::alerts::LimitValue::Float(history_size));
            }
            let (Some(section), Some(model)) = (cfg.section(plugin_name), p.model_mut()) else { continue };
            for (k, v) in section {
                let parsed = match v.parse::<f64>() {
                    Ok(f) => crate::core::alerts::LimitValue::Float(f),
                    Err(_) => crate::core::alerts::LimitValue::List(
                        v.split(',').map(|s| s.trim().to_string()).collect()),
                };
                model.limits.insert(format!("{plugin_name}_{k}"), parsed);
            }
        }
    }

    /// Push config-file settings into the plugins that take them
    /// (currently only power's `[power] kwh_rate`).
    pub fn apply_plugin_config(&self, cfg: &crate::core::config::Config) {
        let rate = cfg.get_float("power", "kwh_rate");
        let mut guard = self.plugins.write().unwrap_or_else(|e| e.into_inner());
        for p in guard.iter_mut() {
            p.set_kwh_rate(rate);
        }
    }
}

impl GlancesStats {
    /// Full snapshot for serialization: plugin name → stats value.
    pub fn snapshot(&self) -> Value {
        let guard = self.plugins.read().unwrap_or_else(|e| e.into_inner());
        Value::Object(guard.iter().map(|p| (p.name().to_string(), p.stats().clone())).collect::<BTreeMap<_, _>>())
    }
}

/// Run a closure, converting a panic into an error instead of unwinding.
fn guarded(f: impl FnOnce()) -> Result<()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(()) => Ok(()),
        Err(_) => Err(super::error::GlancesError::Parse("plugin panicked".into())),
    }
}

/// Fill quicklook's summary from just-updated siblings: cpu/mem/swap
/// copied straight across, load as min1-per-core percent, cpu_name
/// passed through (Null when the cpu plugin has none).
fn fill_quicklook(plugins: &mut [Box<dyn Plugin>]) {
    let num = |name: &str, key: &str| -> Option<f64> {
        plugins
            .iter()
            .find(|p| p.name() == name)?
            .stats()
            .as_object()?
            .get(key)?
            .as_f64()
    };
    let cpu_name = plugins
        .iter()
        .find(|p| p.name() == "cpu")
        .and_then(|p| p.stats().as_object()?.get("cpu_name").cloned())
        .unwrap_or(Value::Null);
    let load = match (num("load", "min1"), num("load", "cpucore")) {
        (Some(min1), Some(cores)) if cores > 0.0 => min1 / cores * 100.0,
        _ => 0.0,
    };
    let summary = [
        ("cpu", num("cpu", "total").unwrap_or(0.0)),
        ("mem", num("mem", "percent").unwrap_or(0.0)),
        ("swap", num("memswap", "percent").unwrap_or(0.0)),
        ("load", load),
    ];
    for p in plugins.iter_mut().filter(|p| p.name() == "quicklook") {
        if let Some(obj) = p.stats_mut().as_object_mut() {
            for (k, v) in summary {
                obj.insert(k.into(), Value::Float(v));
            }
            obj.insert("cpu_name".into(), cpu_name.clone());
        }
    }
}
