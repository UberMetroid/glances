//! Windows platform primitives — M5 placeholders.
//!
//! Real implementation requires FFI to:
//!   kernel32.dll:  GetSystemTimes, GetTickCount64, GlobalMemoryStatusEx
//!   psapi.dll:     GetPerformanceInfo
//!   iphlpapi.dll:  GetIfTable, GetAdaptersInfo
//!   powrprof.dll:  CallNtPowerInformation
//!   ntdll.dll:     NtQuerySystemInformation (processes, etc.)
//!
//! Std-only Rust can declare these as `extern "system"` blocks; linking
//! happens automatically when the symbol name matches a Windows DLL export.
//! For now we ship stubs that return GlancesError::Other.

use crate::core::error::{GlancesError, Result};

pub fn is_windows() -> bool { cfg!(target_os = "windows") }

pub fn read_cpu_times() -> Result<crate::platform::linux::proc_stat::CpuTimes> {
    Err(GlancesError::Other("M5: Windows CPU stats not yet implemented".into()))
}

pub fn read_meminfo() -> Result<crate::platform::linux::proc_meminfo::MemInfo> {
    Err(GlancesError::Other("M5: Windows meminfo not yet implemented".into()))
}

pub fn read_loadavg() -> Result<crate::platform::linux::proc_loadavg::LoadAvg> {
    // Windows doesn't have loadavg; we return an empty record instead.
    Ok(crate::platform::linux::proc_loadavg::LoadAvg::default())
}

pub fn read_uptime() -> Result<f64> {
    Err(GlancesError::Other("M5: Windows uptime not yet implemented".into()))
}
