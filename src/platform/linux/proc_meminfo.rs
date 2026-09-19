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

/// Parse /proc/meminfo content. Tolerates missing keys.
pub fn parse(text: &str) -> Result<MemInfo> {
    let mut out = MemInfo::default();
    for line in text.lines() {
        let (key, kb) = match parse_line(line) {
            Some(kv) => kv,
            None => continue,
        };
        match key {
            "MemTotal" => out.total = kb,
            "MemFree" => out.free = kb,
            "MemAvailable" => out.available = kb,
            "Buffers" => out.buffers = kb,
            "Cached" => out.cached = kb,
            "Shmem" => out.shared = kb,
            "SwapTotal" => out.swap_total = kb,
            "SwapFree" => out.swap_free = kb,
            "HighTotal" => out.high_total = kb,
            "HighFree" => out.high_free = kb,
            "LowTotal" => out.low_total = kb,
            "LowFree" => out.low_free = kb,
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

/// Used memory (total - available). If MemAvailable is missing (older kernels),
/// falls back to total - free - buffers - cached.
pub fn used(self_: &MemInfo) -> u64 {
    if self_.available > 0 && self_.available <= self_.total {
        return self_.total - self_.available;
    }
    self_.total.saturating_sub(self_.free + self_.buffers + self_.cached)
}

pub fn used_mem(info: &MemInfo) -> u64 { used(info) }
pub fn free_mem(info: &MemInfo) -> u64 {
    if info.available > 0 { info.available } else { info.free + info.buffers + info.cached }
}
