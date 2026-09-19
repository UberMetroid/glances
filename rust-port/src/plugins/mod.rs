//! Plugin registry — every plugin module exposes a `register()` function
//! that the stats loop calls during init.

pub mod cpu;
pub mod mem;
pub mod memswap;
pub mod load;
pub mod uptime;
pub mod now;
pub mod system;

use crate::core::stats::GlancesStats;

/// Register all built-in plugins into the given stats container.
/// Each plugin module's `register()` is called here; plugin ordering
/// matches the Python Glances `__init__.py` plugin order.
pub fn register_all(stats: &GlancesStats) {
    cpu::register(stats);
    mem::register(stats);
    memswap::register(stats);
    load::register(stats);
    uptime::register(stats);
    now::register(stats);
    system::register(stats);
}
