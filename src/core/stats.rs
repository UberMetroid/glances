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
}

impl GlancesStats {
    pub fn new(refresh_time: f32) -> Self {
        Self {
            plugins: RwLock::new(Vec::new()),
            refresh_time,
        }
    }

    /// Register a plugin. Order is preserved.
    pub fn register(&self, plugin: Box<dyn Plugin>) {
        let mut guard = self.plugins.write().expect("plugins lock poisoned");
        guard.push(plugin);
    }

    /// Names of all registered plugins in order.
    pub fn plugin_names(&self) -> Vec<&'static str> {
        let guard = self.plugins.read().expect("plugins lock poisoned");
        guard.iter().map(|p| p.name()).collect()
    }

    /// Drive one refresh tick. Calls `update()` on each enabled plugin.
    /// Plugin panics are caught via `catch_unwind` so one bad plugin
    /// cannot kill the loop (matches the plan §4.3 recovery semantics).
    pub fn update(&self) -> Result<()> {
        let mut guard = self.plugins.write().expect("plugins lock poisoned");
        for plugin in guard.iter_mut() {
            let name = plugin.name();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugin.update()
            }));
            match result {
                Ok(Ok(())) => {}
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
        Ok(())
    }

    /// Full snapshot: plugin name -> stats value. This is the value the
    /// web API, XML-RPC `getAll`, and exporters serialize.
    pub fn snapshot(&self) -> Value {
        let mut map = BTreeMap::new();
        let guard = self.plugins.read().expect("plugins lock poisoned");
        for p in guard.iter() {
            map.insert(p.name().to_string(), p.stats().clone());
        }
        Value::Object(map)
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
                crate::exports::write_targets(&stats.snapshot(), &args);
            }
            std::thread::sleep(std::time::Duration::from_secs_f32(refresh_secs));
        }
    });
}
