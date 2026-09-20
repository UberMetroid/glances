//! Plugin registry — every plugin module exposes a `register()` function
//! that the stats loop calls during init.

pub mod cpu;
pub mod percpu;
pub mod irq;
pub mod processcount;
pub mod ip;
pub mod mem;
pub mod memswap;
pub mod load;
pub mod uptime;
pub mod now;
pub mod system;
pub mod fs;
pub mod diskio;
pub mod folders;
pub mod raid;
pub mod network;
pub mod connections;
pub mod ports;
pub mod json;
pub mod containers;
pub mod cloud;
pub mod amps;
pub mod sensors;
pub mod gpu;
pub mod npu;
pub mod wifi;
pub mod mpp;
pub mod alert;
pub mod quicklook;
pub mod help;
pub mod version;
pub mod psutilversion;

use crate::core::stats::GlancesStats;

/// Plugin table: name -> register fn, in the Python Glances
/// `__init__.py` plugin order. Used by `register_all`/`register_filtered`.
const ALL: &[(&str, fn(&GlancesStats))] = &[
    (cpu::NAME, cpu::register),
    (percpu::NAME, percpu::register),
    (irq::NAME, irq::register),
    (processcount::NAME, processcount::register),
    (ip::NAME, ip::register),
    (mem::NAME, mem::register),
    (memswap::NAME, memswap::register),
    (load::NAME, load::register),
    (uptime::NAME, uptime::register),
    (now::NAME, now::register),
    (system::NAME, system::register),
    (fs::NAME, fs::register),
    (diskio::NAME, diskio::register),
    (folders::NAME, folders::register),
    (raid::NAME, raid::register),
    (network::NAME, network::register),
    (connections::NAME, connections::register),
    (ports::NAME, ports::register),
    (containers::NAME, containers::register),
    (cloud::NAME, cloud::register),
    (amps::NAME, amps::register),
    (sensors::NAME, sensors::register),
    (gpu::NAME, gpu::register),
    (npu::NAME, npu::register),
    (wifi::NAME, wifi::register),
    (mpp::NAME, mpp::register),
    (alert::NAME, alert::register),
    (quicklook::NAME, quicklook::register),
    (help::NAME, help::register),
    (version::NAME, version::register),
    (psutilversion::NAME, psutilversion::register),
];

/// Register all built-in plugins into the given stats container.
pub fn register_all(stats: &GlancesStats) {
    register_filtered(stats, &[], &[]);
}

/// Plugins upstream disables by default (glances.conf parity —
/// `[irq] disable=True`). They register only when the user names them
/// in `--enable-plugin` or a config enable list.
const DEFAULT_DISABLED: &[&str] = &[irq::NAME];

/// Register plugins honoring `--enable-plugin`/`--disable-plugin`:
/// a non-empty `enabled` list acts as an allowlist, then `disabled`
/// removes entries. `DEFAULT_DISABLED` plugins additionally require an
/// explicit enable entry. Unknown names are ignored (matching Python,
/// which warns only at the plugin layer).
pub fn register_filtered(stats: &GlancesStats, disabled: &[String], enabled: &[String]) {
    for (name, register) in ALL {
        let explicitly_enabled = enabled.iter().any(|e| e == name);
        if !enabled.is_empty() && !explicitly_enabled { continue; }
        if disabled.iter().any(|d| d == name) { continue; }
        if DEFAULT_DISABLED.contains(name) && !explicitly_enabled { continue; }
        register(stats);
    }
}