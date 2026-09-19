//! Tests for the processcount plugin.

use crate::core::plugin::Plugin;
use crate::core::value::Value;
use crate::plugins::processcount;

#[test]
fn plugin_metadata_and_initial_state() {
    let p = processcount::ProcessCountPlugin::new();
    assert_eq!(p.name(), processcount::NAME);
    assert_eq!(p.name(), "processcount");
    let obj = p.stats().as_object().expect("stats must be an object");
    for k in &["total", "running", "sleeping", "thread", "pid_max"] {
        assert!(obj.contains_key(*k), "missing initial key '{}'", k);
        assert_eq!(obj.get(*k), Some(&Value::Uint(0)));
    }
}

#[test]
fn parse_stat_line_returns_state_and_threads() {
    // Typical line: PID (comm) state ppid ... num_threads ...
    // Field 3 = state, fields 4..19 (16 fields) skipped, field 20 = num_threads.
    // This fixture places 12345 at field 20 (num_threads).
    let line = "1234 (bash) S 1 1234 1234 0 -1 4194304 100 0 0 0 1 0 0 20 0 1 12345 0 0 0";
    let (state, threads) = processcount::parse_stat_line(line).expect("should parse");
    assert_eq!(state, 'S');
    assert_eq!(threads, 12345);
}

#[test]
fn parse_stat_line_handles_comm_with_spaces_and_parens() {
    // The comm field can contain spaces and parens — we must use rfind(')').
    // After rfind(')'), the tail begins with state, then 16 fields, then num_threads.
    // num_threads = 17 is at field 20.
    let line = "9999 (weird (name) here) R 1 9999 9999 0 -1 4194304 100 0 0 0 0 0 0 -20 1 17 42 0 0 0";
    let (state, threads) = processcount::parse_stat_line(line).expect("should parse");
    assert_eq!(state, 'R');
    assert_eq!(threads, 42);
}

#[test]
fn parse_stat_line_truncated_returns_none() {
    // Line ends before the closing ')' — should not panic, just return None.
    let bad = "1234 (bash";
    assert!(processcount::parse_stat_line(bad).is_none());
    // Missing num_threads field.
    let short = "1 (x) S 1 1";
    assert!(processcount::parse_stat_line(short).is_none());
}

#[test]
fn parse_proc_stat_counts_extracts_running_and_blocked() {
    let text = "\
cpu  1 0 0 0 0 0 0 0 0 0
ctxt 1
procs_running 4
procs_blocked 2
intr 1 0
";
    let (running, blocked) = processcount::parse_proc_stat_counts(text);
    assert_eq!(running, 4);
    assert_eq!(blocked, 2);
}

#[test]
fn parse_proc_stat_counts_handles_missing_keys() {
    // No procs_* lines — should fall back to (0, 0), not panic.
    let (running, blocked) = processcount::parse_proc_stat_counts("cpu 1 0 0 0 0 0 0 0 0 0\n");
    assert_eq!(running, 0);
    assert_eq!(blocked, 0);
}

#[test]
fn parse_proc_stat_counts_tolerates_garbage_numbers() {
    let (running, blocked) = processcount::parse_proc_stat_counts(
        "procs_running notanumber\nprocs_blocked 7\n"
    );
    assert_eq!(running, 0);  // bad number parses as 0
    assert_eq!(blocked, 7);
}

#[test]
fn update_on_linux_writes_numeric_keys() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = processcount::ProcessCountPlugin::new();
    p.update().expect("processcount update should succeed on Linux");
    let obj = p.stats().as_object().expect("stats must be an object");
    let total = obj.get("total").and_then(|v| match v {
        Value::Uint(n) => Some(*n),
        _ => None,
    }).expect("missing 'total'");
    assert!(total > 0, "total should be > 0 on a live system, got {}", total);
    // pid_max should be > 0 on any Linux.
    let pid_max = obj.get("pid_max").and_then(|v| match v {
        Value::Uint(n) => Some(*n),
        _ => None,
    }).unwrap_or(0);
    assert!(pid_max > 0, "pid_max should be > 0 on Linux, got {}", pid_max);
}

#[test]
fn reset_restores_initial_object() {
    let mut p = processcount::ProcessCountPlugin::new();
    // Clobber stats to non-zero.
    if let Some(obj) = p.stats_mut().as_object_mut() {
        obj.insert("total".into(), Value::Uint(999));
    }
    p.reset();
    let obj = p.stats().as_object().unwrap();
    assert_eq!(obj.get("total"), Some(&Value::Uint(0)));
}