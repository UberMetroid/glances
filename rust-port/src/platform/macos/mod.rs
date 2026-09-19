//! macOS platform primitives — M4 placeholders.
//!
//! Real implementation requires FFI to libc (sysctlbyname, host_info,
//! mach APIs). When M4 is fully implemented this module gains:
//! - sysctl.rs (kern.cp_times, hw.memsize, vm.swapusage, etc.)
//! - host_info.rs (host_statistics64 for CPU stats)
//! - mach.rs (host_processor_info, task_info for per-process)
//! - iokit.rs (IOKit for sensors)
//!
//! For now we ship stubs that return GlancesError::Other so callers fail
//! gracefully on macOS hosts without breaking the Linux build.

use crate::core::error::{GlancesError, Result};

pub fn is_macos() -> bool { cfg!(target_os = "macos") }

pub fn read_cpu_times() -> Result<crate::platform::linux::proc_stat::CpuTimes> {
    Err(GlancesError::Other("M4: macOS CPU stats not yet implemented".into()))
}

pub fn read_meminfo() -> Result<crate::platform::linux::proc_meminfo::MemInfo> {
    Err(GlancesError::Other("M4: macOS meminfo not yet implemented".into()))
}

pub fn read_loadavg() -> Result<crate::platform::linux::proc_loadavg::LoadAvg> {
    Err(GlancesError::Other("M4: macOS loadavg not yet implemented".into()))
}

pub fn read_uptime() -> Result<f64> {
    Err(GlancesError::Other("M4: macOS uptime not yet implemented".into()))
}
