//! Process counts — total / running / sleeping / thread counts.
//!
//! Mirrors `glances/plugins/processcount/__init__.py`. Linux-only.
//! `total` is the count of numeric PIDs visible under `/proc`. `running`
//! and `sleeping` come from the third field of `/proc/<pid>/stat` (state
//! char). `thread` is the total number of threads across all processes
//! (field 20 in `/proc/<pid>/stat`).

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::{GlancesError, Result};
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "processcount";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(ProcessCountPlugin::new()));
}

pub struct ProcessCountPlugin { base: GlancesPluginModel }

impl Default for ProcessCountPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessCountPlugin {
    pub fn new() -> Self {
        let mut m: BTreeMap<String, Value> = BTreeMap::new();
        for k in &["total", "running", "sleeping", "thread", "pid_max"] {
            m.insert(k.to_string(), Value::Uint(0));
        }
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

/// Count numeric entries in `/proc` (these are the per-PID directories).
pub fn count_pids() -> Result<u64> {
    let entries = fs::read_dir("/proc").map_err(GlancesError::Io)?;
    let mut n: u64 = 0;
    for e in entries.flatten() {
        if e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
            n += 1;
        }
    }
    Ok(n)
}

/// Read `/proc/<pid>/stat` and return (state_char, num_threads).
/// Field 3 (state) and field 20 (num_threads) per proc(5).
/// `comm` may contain spaces/parens, so the line ends with `) <state> ...`.
pub fn parse_stat_line(line: &str) -> Option<(char, u64)> {
    // The PID is the leading whitespace-delimited token.
    let after_pid_space = line.find(' ')?;
    let rest = &line[after_pid_space + 1..];
    // Now find the matching ')' that closes the comm field.
    let close = rest.rfind(')')?;
    let tail = rest[close + 1..].trim_start();
    let mut it = tail.split_whitespace();
    let state = it.next()?.chars().next()?;
    // After state (field 3), num_threads is field 20 → 16 fields to skip
    // (ppid, pgrp, session, tty_nr, tpgid, flags, minflt, cminflt, majflt,
    // cmajflt, utime, stime, cutime, cstime, priority, nice).
    let mut skipped = 0;
    while skipped < 16 {
        it.next()?;
        skipped += 1;
    }
    let threads = it.next()?.parse::<u64>().ok()?;
    Some((state, threads))
}

/// Read /proc/stat and pull `procs_running` + `procs_blocked`.
pub fn parse_proc_stat_counts(text: &str) -> (u64, u64) {
    let mut running = 0;
    let mut blocked = 0;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("procs_running ") {
            running = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("procs_blocked ") {
            blocked = rest.trim().parse().unwrap_or(0);
        }
    }
    (running, blocked)
}

/// Aggregate counts by walking `/proc`. Returns (total, running, sleeping,
/// thread). On IO errors, falls back to the supplied fallback counts
/// (typically from /proc/stat) so the plugin never goes blank.
pub fn aggregate(fallback_running: u64, fallback_blocked: u64) -> (u64, u64, u64, u64) {
    let entries = match fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return (0, fallback_running, fallback_blocked, 0),
    };
    let mut total: u64 = 0;
    let mut running: u64 = 0;
    let mut sleeping: u64 = 0;
    let mut threads: u64 = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.chars().all(|c| c.is_ascii_digit()) { continue; }
        total += 1;
        let path = format!("/proc/{}/stat", name);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Some((state, t)) = parse_stat_line(&text) {
            threads += t;
            match state {
                'R' => running += 1,
                'S' | 'I' => sleeping += 1,
                'D' | 'W' => {} // uninterruptible / paging — neither run nor sleep here
                _ => {}
            }
        }
    }
    (total, running, sleeping, threads)
}

/// Read /proc/sys/kernel/pid_max. Falls back to 0 on error.
pub fn read_pid_max() -> u64 {
    fs::read_to_string("/proc/sys/kernel/pid_max")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

impl Plugin for ProcessCountPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }

    fn history_items(&self) -> &[&'static str] { &["total", "running", "sleeping", "thread"] }
    /// Upstream yields the empty init value over SNMP (per-process
    /// enumeration has no standard MIB); reset keeps that outcome
    /// without the unsupported debug log every tick.
    fn update_snmp(&mut self, _ctx: &crate::core::snmp::SnmpCtx) -> Result<()> {
        self.reset();
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        if !cfg!(target_os = "linux") {
            if let Some(obj) = self.base.stats.as_object_mut() {
                for k in &["total", "running", "sleeping", "thread", "pid_max"] {
                    obj.insert((*k).into(), Value::Uint(0));
                }
            }
            return Ok(());
        }
        // Fallback counts from /proc/stat — used when /proc scan fails.
        let stat_text = fs::read_to_string("/proc/stat").unwrap_or_default();
        let (fr, fb) = parse_proc_stat_counts(&stat_text);
        let (total, running, sleeping, thread) = aggregate(fr, fb);
        let pid_max = read_pid_max();
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("total".into(), Value::Uint(total));
            obj.insert("running".into(), Value::Uint(running));
            obj.insert("sleeping".into(), Value::Uint(sleeping));
            obj.insert("thread".into(), Value::Uint(thread));
            obj.insert("pid_max".into(), Value::Uint(pid_max));
        }
        Ok(())
    }
}