//! /sys/class/net reader — per-interface operational state, speed, and stats.
//!
//! For tx/rx bytes/packets we read /proc/net/dev (see proc_net_dev.rs).
//! This module reads /sys/class/net/<iface>/{operstate,speed} for state +
//! link speed. Both files may be missing on some systems (loopback, tunnels).

use std::fs;
use std::path::Path;

use crate::core::error::{GlancesError, Result};

/// Per-interface metadata from /sys/class/net.
#[derive(Debug, Default, Clone)]
pub struct IfaceMeta {
    pub operstate: String, // "up", "down", "unknown", "notpresent", ...
    pub speed_mbps: Option<u64>, // link speed; None if unknown
    pub is_physical: bool, // true if /sys/class/net/<iface>/device exists
}

pub fn list_interfaces() -> Result<Vec<String>> {
    let entries = fs::read_dir("/sys/class/net").map_err(GlancesError::Io)?;
    let mut out = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.is_empty() || name.starts_with('.') { continue; }
        out.push(name);
    }
    out.sort();
    Ok(out)
}

pub fn read_meta(iface: &str) -> Result<IfaceMeta> {
    let base = Path::new("/sys/class/net").join(iface);
    let operstate = fs::read_to_string(base.join("operstate"))
        .unwrap_or_else(|_| "unknown".into()).trim().to_string();
    let mut speed_mbps = None;
    if let Ok(speed_str) = fs::read_to_string(base.join("speed"))
        && let Ok(speed) = speed_str.trim().parse::<u64>() {
            // speed == -1 is reported as max u64 by some kernels.
            if speed > 0 && speed < u64::MAX / 2 {
                speed_mbps = Some(speed);
            }
        }
    Ok(IfaceMeta {
        operstate,
        speed_mbps,
        is_physical: base.join("device").exists(),
    })
}
