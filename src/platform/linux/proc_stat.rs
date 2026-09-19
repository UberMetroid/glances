//! /proc/stat parser — provides CPU times, context switches, interrupts.
//!
//! Format (Linux):
//!   cpu  user nice system idle iowait irq softirq steal guest guest_nice
//!   cpu0 user nice system idle iowait irq softirq steal guest guest_nice
//!   ...
//!   ctxt <total context switches>
//!   intr <total interrupts>
//!   btime <boot time in seconds since epoch>
//!   processes <total forks since boot>

use std::fs;
use std::io;

use crate::core::error::{GlancesError, Result};

/// Per-CPU or aggregate time breakdown (units: USER_HZ jiffies).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
    pub guest: u64,
    pub guest_nice: u64,
}

impl CpuTimes {
    /// Total non-idle time.
    pub fn busy(&self) -> u64 {
        self.user + self.nice + self.system + self.iowait
            + self.irq + self.softirq + self.steal
            + self.guest + self.guest_nice
    }
    /// Total time (busy + idle).
    pub fn total(&self) -> u64 { self.busy() + self.idle }

    /// Per-field difference from an earlier snapshot. Each field is a
    /// monotonic cumulative counter; saturating subtraction turns a
    /// counter rollback/reset into 0 for that field instead of wrapping.
    pub fn delta(&self, prev: &CpuTimes) -> CpuTimes {
        CpuTimes {
            user: self.user.saturating_sub(prev.user),
            nice: self.nice.saturating_sub(prev.nice),
            system: self.system.saturating_sub(prev.system),
            idle: self.idle.saturating_sub(prev.idle),
            iowait: self.iowait.saturating_sub(prev.iowait),
            irq: self.irq.saturating_sub(prev.irq),
            softirq: self.softirq.saturating_sub(prev.softirq),
            steal: self.steal.saturating_sub(prev.steal),
            guest: self.guest.saturating_sub(prev.guest),
            guest_nice: self.guest_nice.saturating_sub(prev.guest_nice),
        }
    }
}

/// Whole-file snapshot of /proc/stat.
#[derive(Debug, Default, Clone)]
pub struct ProcStat {
    pub total: CpuTimes,
    pub per_cpu: Vec<CpuTimes>,
    pub ctxt: u64,
    pub intr: u64,
    pub btime: u64,
    pub processes: u64,
}

pub fn read() -> Result<ProcStat> {
    let text = fs::read_to_string("/proc/stat").map_err(GlancesError::Io)?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<ProcStat> {
    let mut out = ProcStat::default();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let key = match parts.next() {
            Some(k) => k,
            None => continue,
        };
        if key == "cpu" {
            out.total = parse_cpu_times(&mut parts);
        } else if key.starts_with("cpu") && key.len() > 3 {
            // per-CPU: cpu0, cpu1, ...
            if key.as_bytes()[3].is_ascii_digit() {
                out.per_cpu.push(parse_cpu_times(&mut parts));
            }
        } else if key == "ctxt" {
            out.ctxt = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if key == "intr" {
            out.intr = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if key == "btime" {
            out.btime = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if key == "processes" {
            out.processes = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        }
        // Ignore other lines (intr, softirq, procs_running, procs_blocked, etc.)
    }
    Ok(out)
}

fn parse_cpu_times<'a, I: Iterator<Item = &'a str>>(parts: &mut I) -> CpuTimes {
    fn next_u64<'a, I: Iterator<Item = &'a str>>(parts: &mut I) -> u64 {
        parts.next().and_then(|s| s.parse().ok()).unwrap_or(0)
    }
    CpuTimes {
        user: next_u64(parts),
        nice: next_u64(parts),
        system: next_u64(parts),
        idle: next_u64(parts),
        iowait: next_u64(parts),
        irq: next_u64(parts),
        softirq: next_u64(parts),
        steal: next_u64(parts),
        guest: next_u64(parts),
        guest_nice: next_u64(parts),
    }
}

pub fn io_error_is_not_found(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::NotFound
}
