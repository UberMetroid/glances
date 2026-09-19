//! /proc/uptime and /proc/stat::btime — boot time and uptime.

use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::core::error::{GlancesError, Result};

/// Read uptime in seconds (fractional).
pub fn read_uptime() -> Result<f64> {
    let text = fs::read_to_string("/proc/uptime").map_err(GlancesError::Io)?;
    let first = text.lines().next().ok_or_else(|| GlancesError::Parse("empty uptime".into()))?;
    let secs: f64 = first.split_whitespace().next()
        .ok_or_else(|| GlancesError::Parse("missing uptime".into()))?
        .parse()
        .map_err(|_| GlancesError::Parse("bad uptime".into()))?;
    Ok(secs)
}

/// Read boot time as SystemTime, derived from /proc/stat::btime.
pub fn read_boot_time() -> Result<SystemTime> {
    let text = fs::read_to_string("/proc/stat").map_err(GlancesError::Io)?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("btime ") {
            let secs: u64 = rest.trim().parse().map_err(|_| GlancesError::Parse("bad btime".into()))?;
            return Ok(UNIX_EPOCH + Duration::from_secs(secs));
        }
    }
    Err(GlancesError::Parse("btime not found".into()))
}

/// Convert uptime seconds into a Duration.
pub fn uptime_duration() -> Result<Duration> {
    Ok(Duration::from_secs_f64(read_uptime()?))
}
