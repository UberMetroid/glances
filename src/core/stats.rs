//! GlancesStats — plugin registry + refresh loop.
//!
//! M1 implementation: minimal registry + update() driver. Full snapshot
//! publishing + exporter fan-out lands in M12+.

use std::sync::RwLock;

use super::error::Result;
use super::plugin::Plugin;

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
}
