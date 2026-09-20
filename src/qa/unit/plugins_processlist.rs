//! Tests for the processlist plugin — per-process sampling helpers.

use std::collections::HashMap;

use crate::plugins::processlist::{
    build_user_map, parse_stat_fields, register, sample_to_value, status_name, ProcSample,
};

// stat tail: R ppid pgrp sess tty tpgid flags minflt cminflt majflt cmajflt
// utime stime cutime cstime prio nice threads + 19 trailing fields (last = cpu 3).
const STAT_LINE: &str = "1234 (my prog (x)) R 1 2 3 4 5 6 7 8 9 10 100 200 0 0 20 0 4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 3";

#[test]
fn parse_stat_fields_handles_paren_in_comm() {
    let (comm, state, utime, stime, nice, threads, cpu) =
        parse_stat_fields(STAT_LINE).expect("must parse");
    assert_eq!(comm, "my prog (x)");
    assert_eq!(state, 'R');
    assert_eq!(utime, 100);
    assert_eq!(stime, 200);
    assert_eq!(nice, 0);
    assert_eq!(threads, 4);
    assert_eq!(cpu, 3);
}

#[test]
fn parse_stat_fields_rejects_short_lines() {
    assert!(parse_stat_fields("1 (a) R").is_none());
    assert!(parse_stat_fields("not a stat line").is_none());
    assert!(parse_stat_fields("").is_none());
}

#[test]
fn status_name_maps_states() {
    assert_eq!(status_name('R'), "running");
    assert_eq!(status_name('S'), "sleeping");
    assert_eq!(status_name('D'), "disk-sleep");
    assert_eq!(status_name('Z'), "zombie");
    assert_eq!(status_name('Q'), "unknown");
}

#[test]
fn user_map_parses_passwd() {
    let map = build_user_map();
    assert!(!map.is_empty(), "/etc/passwd must yield users");
}

fn sample() -> ProcSample {
    ProcSample {
        pid: 42,
        name: "demo".into(),
        cmdline: "demo --flag".into(),
        username: "root".into(),
        num_threads: 3,
        state: 'S',
        nice: 0,
        cpu_percent: 12.345,
        memory_percent: 0.5,
        rss: 1024,
        vms: 2048,
        utime: 10,
        stime: 5,
        read_bytes: 100,
        write_bytes: 200,
        cpu_num: 1,
    }
}

#[test]
fn sample_to_value_carries_all_keys() {
    let v = sample_to_value(&sample());
    let obj = v.as_object().expect("object");
    for k in [
        "pid",
        "name",
        "cmdline",
        "username",
        "num_threads",
        "cpu_percent",
        "memory_percent",
        "memory_info",
        "status",
        "nice",
        "cpu_times",
        "io_counters",
        "cpu_num",
    ] {
        assert!(obj.contains_key(k), "missing key {}", k);
    }
    assert_eq!(obj.get("status").and_then(|v| v.as_str()), Some("sleeping"));
    let mem = obj
        .get("memory_info")
        .and_then(|v| v.as_object())
        .expect("memory_info");
    assert_eq!(mem.get("rss").and_then(|v| v.as_f64()), Some(1024.0));
}

#[test]
fn sample_all_second_tick_prunes_and_percents() {
    // Live /proc walk: must not panic; pids observed twice get percents.
    let mut prev = HashMap::new();
    let first = crate::plugins::processlist::sample_all(&mut prev);
    assert!(!first.is_empty());
    assert!(first.iter().all(|p| p.cpu_percent == 0.0));
    let second = crate::plugins::processlist::sample_all(&mut prev);
    assert!(!second.is_empty());
    assert!(second.iter().all(|p| p.cpu_percent >= 0.0));
}

#[test]
fn register_plugin_appears_in_stats() {
    let s = crate::core::stats::GlancesStats::new(1.0);
    register(&s);
    assert!(s.plugin_names().contains(&"processlist"));
}
