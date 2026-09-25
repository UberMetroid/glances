//! Plugin registry — every plugin module exposes a `register()` function
//! that the stats loop calls during init.

pub mod cpu;
pub mod percpu;
pub mod irq;
pub mod processcount;
pub mod processlist;
pub mod programlist;
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
pub mod smart;
pub mod vms;
pub mod sensors;
pub mod gpu;
pub mod gpu_drm;
pub mod gpu_format;
pub mod gpu_nvidia;
pub mod gpu_proc;
pub mod gpu_sysfs;
pub mod fs_rootfs;
pub mod npu;
pub mod power;
pub mod wifi;
pub mod mpp;
pub mod pressure;
pub mod alert;
pub mod quicklook;
pub mod help;
pub mod version;
pub mod psutilversion;

use crate::core::stats::GlancesStats;

/// One plugin table row: name -> register fn, in the Python Glances
/// `__init__.py` plugin order. Used by `register_all`/`register_filtered`.
type PluginEntry = (&'static str, fn(&GlancesStats));
const ALL: &[PluginEntry] = &[
    (cpu::NAME, cpu::register),
    (percpu::NAME, percpu::register),
    (irq::NAME, irq::register),
    (processcount::NAME, processcount::register),
    (processlist::NAME, processlist::register),
    (programlist::NAME, programlist::register),
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
    (smart::NAME, smart::register),
    (vms::NAME, vms::register),
    (sensors::NAME, sensors::register),
    (gpu::NAME, gpu::register),
    (npu::NAME, npu::register),
    (power::NAME, power::register),
    (wifi::NAME, wifi::register),
    (mpp::NAME, mpp::register),
    (pressure::NAME, pressure::register),
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

/// Names of all built-in plugins in registry order (`--modules-list`).
pub fn plugin_names() -> Vec<&'static str> {
    ALL.iter().map(|(name, _)| *name).collect()
}

/// Plugins upstream disables by default (glances.conf parity —
/// `[irq] disable=True`). They register only when the user names them
/// in `--enable-plugin` or a config enable list.
const DEFAULT_DISABLED: &[&str] = &[irq::NAME];

/// Register plugins honoring `--enable-plugin`/`--disable-plugin`
/// (upstream `stats.py` parity): the set is narrowed ONLY when
/// `disabled` contains `all`; a bare `enabled` list merely switches on
/// `DEFAULT_DISABLED` plugins and never disables the rest. Named
/// `disabled` entries always win. Unknown names are ignored (matching
/// Python, which warns only at the plugin layer).
pub fn register_filtered(stats: &GlancesStats, disabled: &[String], enabled: &[String]) {
    let disable_all = disabled.iter().any(|d| d == "all");
    for (name, register) in ALL {
        let explicitly_enabled = enabled.iter().any(|e| e == name);
        if disable_all && !explicitly_enabled { continue; }
        if disabled.iter().any(|d| d == name) { continue; }
        if DEFAULT_DISABLED.contains(name) && !explicitly_enabled { continue; }
        register(stats);
    }
}