//! Plugin registry: module tree plus ordered registration.
//!
//! Every data source exposes `register`; the stats loop registers
//! the filtered set once at startup. Table order is load-bearing —
//! `--modules-list` and `/api/4/pluginslist` expose it.

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
pub mod net_role;
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

/// Name plus register fn, in canonical order. Helper modules (json,
/// gpu_*, fs_rootfs) expose no plugin and stay out of this table.
type Entry = (&'static str, fn(&GlancesStats));
const ALL: &[Entry] = &[
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

/// Plugins that stay off unless explicitly enabled.
const DEFAULT_DISABLED: &[&str] = &[irq::NAME];

/// Register every plugin in canonical order.
pub fn register_all(stats: &GlancesStats) {
    register_filtered(stats, &[], &[]);
}

/// Canonical plugin names (`--modules-list`).
pub fn plugin_names() -> Vec<&'static str> {
    ALL.iter().map(|(name, _)| *name).collect()
}

/// Register honoring `--enable-plugin`/`--disable-plugin`. The set
/// narrows ONLY when `disabled` holds `all` (plus explicit enables);
/// a bare enable list merely switches on default-disabled plugins
/// and never drops the rest. Named disables always win; unknown
/// names are silently ignored.
pub fn register_filtered(stats: &GlancesStats, disabled: &[String], enabled: &[String]) {
    let narrow = disabled.iter().any(|d| d == "all");
    for (name, register) in ALL {
        let wanted = enabled.iter().any(|e| e == name);
        let banned = disabled.iter().any(|d| d == name);
        let needs_opt_in = DEFAULT_DISABLED.contains(name) && !wanted;
        if banned || narrow && !wanted || needs_opt_in {
            continue;
        }
        register(stats);
    }
}
