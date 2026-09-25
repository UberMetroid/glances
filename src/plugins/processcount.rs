//! Process census — total/running/sleeping/thread counts, Linux-only.
//!
//! `total` counts numeric PIDs under /proc. Running/sleeping come from
//! each stat line's state char; `thread` sums field 20 (num_threads).
//! Vanished processes and unreadable files skip silently mid-scan, and
//! a dead /proc falls back to the /proc/stat counters.

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
    fn default() -> Self { Self::new() }
}

impl ProcessCountPlugin {
    pub fn new() -> Self {
        let m: BTreeMap<String, Value> = ["total", "running", "sleeping", "thread", "pid_max"]
            .into_iter()
            .map(|k| (k.to_string(), Value::Uint(0)))
            .collect();
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

/// Numeric entries under /proc (the per-PID directories).
pub fn count_pids() -> Result<u64> {
    let mut n: u64 = 0;
    for e in fs::read_dir("/proc").map_err(GlancesError::Io)?.flatten() {
        if e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
            n += 1;
        }
    }
    Ok(n)
}

/// One stat line → (state char, thread count): field 3 and field 20
/// per proc(5). The line splits on the FIRST space (past the PID) and
/// the LAST `)` (comm may itself hold spaces and parens); state is
/// the next token, then 16 fields are skipped to reach num_threads.
pub fn parse_stat_line(line: &str) -> Option<(char, u64)> {
    let after_pid = line.find(' ').map(|i| &line[i + 1..])?;
    let tail = after_pid.rfind(')').map(|i| after_pid[i + 1..].trim_start())?;
    let mut fields = tail.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let threads = fields.nth(16)?.parse::<u64>().ok()?;
    Some((state, threads))
}

/// `procs_running` + `procs_blocked` from /proc/stat text (the
/// whole-scan fallback pair).
pub fn parse_proc_stat_counts(text: &str) -> (u64, u64) {
    let mut counts = (0, 0);
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("procs_running ") {
            counts.0 = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("procs_blocked ") {
            counts.1 = v.trim().parse().unwrap_or(0);
        }
    }
    counts
}

/// Walk /proc → (total, running, sleeping, threads). Only R runs and
/// only S/I sleep here (D/W and the rest count toward neither).
pub fn aggregate(fallback_running: u64, fallback_blocked: u64) -> (u64, u64, u64, u64) {
    let entries = match fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return (0, fallback_running, fallback_blocked, 0),
    };
    let (mut total, mut running, mut sleeping, mut threads) = (0, 0, 0, 0);
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        total += 1;
        let text = match fs::read_to_string(format!("/proc/{name}/stat")) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Some((state, t)) = parse_stat_line(&text) {
            threads += t;
            match state {
                'R' => running += 1,
                'S' | 'I' => sleeping += 1,
                _ => {}
            }
        }
    }
    (total, running, sleeping, threads)
}

/// Kernel pid_max, 0 when unreadable.
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
    /// No standard MIB covers process enumeration — reset (empty init
    /// values) without the unsupported debug log every tick.
    fn update_snmp(&mut self, _ctx: &crate::core::snmp::SnmpCtx) -> Result<()> {
        self.reset();
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        let stat_text = fs::read_to_string("/proc/stat").unwrap_or_default();
        let (fr, fb) = parse_proc_stat_counts(&stat_text);
        let (total, running, sleeping, thread) = aggregate(fr, fb);
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("total".into(), Value::Uint(total));
            obj.insert("running".into(), Value::Uint(running));
            obj.insert("sleeping".into(), Value::Uint(sleeping));
            obj.insert("thread".into(), Value::Uint(thread));
            obj.insert("pid_max".into(), Value::Uint(read_pid_max()));
        }
        Ok(())
    }
}
