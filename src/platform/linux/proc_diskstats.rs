//! /proc/diskstats parser — per-device read/write byte counts and I/O latency.
//!
//! Format (Linux):
//!   major minor name [reads_completed reads_merged sectors_read time_reading
//!    writes_completed writes_merged sectors_written time_writing
//!    io_in_progress time_io weighted_time_io] [discards_completed ...]
//!
//! Field positions (1-indexed in kernel docs):
//!   1: reads completed
//!   3: sectors read
//!   5: writes completed
//!   7: sectors written

use std::fs;

use crate::core::error::{GlancesError, Result};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct DiskStats {
    pub name: String,
    pub reads_completed: u64,
    pub sectors_read: u64,
    pub writes_completed: u64,
    pub sectors_written: u64,
}

pub fn read() -> Result<Vec<DiskStats>> {
    let text = fs::read_to_string("/proc/diskstats").map_err(GlancesError::Io)?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Vec<DiskStats>> {
    let mut out = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Older kernels emit 14 fields; newer ones extend with discard stats.
        if parts.len() < 14 { continue; }
        let name = parts[2].to_string();
        // Skip partitions (those with a "p" in their name pattern, e.g. sda1).
        if name.starts_with("ram") || name.starts_with("loop") { continue; }
        let reads_completed: u64 = parts[3].parse().unwrap_or(0);
        let sectors_read: u64 = parts[5].parse().unwrap_or(0);
        let writes_completed: u64 = parts[7].parse().unwrap_or(0);
        let sectors_written: u64 = parts[9].parse().unwrap_or(0);
        out.push(DiskStats { name, reads_completed, sectors_read, writes_completed, sectors_written });
    }
    Ok(out)
}

/// Sector size in bytes (Linux standard). For 4K-sector devices this would
/// be 512 (the kernel still reports 512-byte sectors in /proc/diskstats).
pub const SECTOR_SIZE: u64 = 512;

pub fn read_bytes(d: &DiskStats) -> u64 { d.sectors_read * SECTOR_SIZE }
pub fn write_bytes(d: &DiskStats) -> u64 { d.sectors_written * SECTOR_SIZE }
