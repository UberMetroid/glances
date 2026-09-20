//! /proc/net/dev parser — per-interface RX/TX byte/packet counters.
//!
//! Format (Linux):
//!   Inter-|   Receive                                                |  Transmit
//!    face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
//!          0       1    2    3    4    5       6          7       8       9   10   11    12    13    14
//!     eth0: 12345   100    0    0    0    0        0          0    6789    50    0    0    0    0    0

use std::fs;

use crate::core::error::{GlancesError, Result};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct IfaceStats {
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errors: u64,
    pub rx_drop: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errors: u64,
    pub tx_drop: u64,
}

pub fn read() -> Result<Vec<(String, IfaceStats)>> {
    let text = fs::read_to_string("/proc/net/dev").map_err(GlancesError::Io)?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Vec<(String, IfaceStats)>> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    // First two lines are the header.
    let _ = lines.next();
    let _ = lines.next();
    for line in lines {
        if line.trim().is_empty() { continue; }
        if let Some(entry) = parse_line(line) {
            out.push(entry);
        }
    }
    Ok(out)
}

fn parse_line(line: &str) -> Option<(String, IfaceStats)> {
    let colon = line.find(':')?;
    let iface = line[..colon].trim().to_string();
    let rest = line[colon + 1..].trim();
    // Positional parse of the first 12 columns. A `filter_map` that
    // skips non-numeric tokens would silently shift every later column
    // left — a malformed token must drop the line, not corrupt fields.
    let mut nums = [0u64; 12];
    let mut it = rest.split_whitespace();
    for slot in nums.iter_mut() {
        *slot = it.next().and_then(|s| s.parse().ok())?;
    }
    Some((iface, IfaceStats {
        rx_bytes: nums[0],
        rx_packets: nums[1],
        rx_errors: nums[2],
        rx_drop: nums[3],
        tx_bytes: nums[8],
        tx_packets: nums[9],
        tx_errors: nums[10],
        tx_drop: nums[11],
    }))
}
