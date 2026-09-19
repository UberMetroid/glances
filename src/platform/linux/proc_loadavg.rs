//! /proc/loadavg parser — 1/5/15-minute load averages plus running/total procs.

use std::fs;

use crate::core::error::{GlancesError, Result};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct LoadAvg {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    /// Running processes (R state) / total processes.
    pub running: u32,
    pub total: u32,
    /// Last PID (used as a sanity check; newer kernels).
    pub last_pid: u32,
}

pub fn read() -> Result<LoadAvg> {
    let text = fs::read_to_string("/proc/loadavg").map_err(GlancesError::Io)?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<LoadAvg> {
    let first = text.lines().next().ok_or_else(|| GlancesError::Parse("empty loadavg".into()))?;
    let mut parts = first.split_whitespace();
    let load1 = parts.next().ok_or_else(|| GlancesError::Parse("missing load1".into()))?
        .parse().map_err(|_| GlancesError::Parse("bad load1".into()))?;
    let load5 = parts.next().ok_or_else(|| GlancesError::Parse("missing load5".into()))?
        .parse().map_err(|_| GlancesError::Parse("bad load5".into()))?;
    let load15 = parts.next().ok_or_else(|| GlancesError::Parse("missing load15".into()))?
        .parse().map_err(|_| GlancesError::Parse("bad load15".into()))?;
    // Fourth field: "running/total"
    let fourth = parts.next().ok_or_else(|| GlancesError::Parse("missing procs".into()))?;
    let mut slash = fourth.split('/');
    let running: u32 = slash.next().unwrap_or("0").parse().unwrap_or(0);
    let total: u32 = slash.next().unwrap_or("0").parse().unwrap_or(0);
    let last_pid: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(LoadAvg { load1, load5, load15, running, total, last_pid })
}
