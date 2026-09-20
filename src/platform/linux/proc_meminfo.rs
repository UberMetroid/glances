//! /proc/meminfo parser — RAM and (Linux) buffers/cache breakdown.

use std::fs;

use crate::core::error::{GlancesError, Result};

#[derive(Debug, Default, Clone)]
pub struct MemInfo {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub buffers: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub shared: u64,
    /// Total highmem (32-bit only; zero on 64-bit).
    pub high_total: u64,
    pub high_free: u64,
    /// Total lowmem.
    pub low_total: u64,
    pub low_free: u64,
}

pub fn read() -> Result<MemInfo> {
    let text = fs::read_to_string("/proc/meminfo").map_err(GlancesError::Io)?;
    parse(&text)
}

/// Parse /proc/meminfo content. All returned fields are **bytes**
/// (psutil parity): the kernel reports kB, converted here once so every
/// consumer gets the right unit.
pub fn parse(text: &str) -> Result<MemInfo> {
    let mut out = MemInfo::default();
    for line in text.lines() {
        let (key, kb) = match parse_line(line) {
            Some(kv) => kv,
            None => continue,
        };
        let bytes = kb.saturating_mul(1024);
        match key {
            "MemTotal" => out.total = bytes,
            "MemFree" => out.free = bytes,
            "MemAvailable" => out.available = bytes,
            "Buffers" => out.buffers = bytes,
            "Cached" => out.cached = bytes,
            "Shmem" => out.shared = bytes,
            "SwapTotal" => out.swap_total = bytes,
            "SwapFree" => out.swap_free = bytes,
            "HighTotal" => out.high_total = bytes,
            "HighFree" => out.high_free = bytes,
            "LowTotal" => out.low_total = bytes,
            "LowFree" => out.low_free = bytes,
            _ => {}
        }
    }
    Ok(out)
}

/// Parse a single line of the form "KeyName:    12345 kB".
/// Returns (key, kilobytes).
fn parse_line(line: &str) -> Option<(&str, u64)> {
    let mut parts = line.split(':');
    let key = parts.next()?.trim();
    let value = parts.next()?.trim();
    // Value may have "kB" suffix.
    let kb_str = value.trim_end_matches("kB").trim();
    let kb: u64 = kb_str.parse().ok()?;
    Some((key, kb))
}

/// Used memory — psutil formula: `total - free - buffers - cached`.
/// (This is intentionally *not* `total - available`: psutil's `used`
/// counts reclaimable cache as used; `percent` below uses `available`.)
pub fn used(self_: &MemInfo) -> u64 {
    self_.total.saturating_sub(self_.free + self_.buffers + self_.cached)
}

/// Percent used — psutil formula `(total - available) / total * 100`,
/// falling back to the used-formula base when MemAvailable is missing.
pub fn percent_used(info: &MemInfo) -> f64 {
    if info.total == 0 { return 0.0; }
    let avail = if info.available > 0 && info.available <= info.total {
        info.available
    } else {
        info.free + info.buffers + info.cached
    };
    (info.total.saturating_sub(avail) as f64 / info.total as f64) * 100.0
}

pub fn used_mem(info: &MemInfo) -> u64 { used(info) }
/// Free memory is raw MemFree — psutil's `vm.free` parity. `available`
/// (MemAvailable) is a separate, larger metric the plugin exposes
/// separately; conflating them makes `free` report the wrong number.
pub fn free_mem(info: &MemInfo) -> u64 { info.free }
