//! Tests for the processlist plugin — per-process sampling helpers.

use std::collections::HashMap;

use crate::plugins::processlist::{
    build_user_map, parse_io_text, parse_stat_fields, parse_statm_text, parse_status_text,
    register, sample_to_value, status_name, ProcSample,
};

// stat tail: R ppid pgrp sess tty tpgid flags minflt cminflt majflt cmajflt
// utime stime cutime cstime prio nice threads + 19 trailing fields (last = cpu 3).
const STAT_LINE: &str = "1234 (my prog (x)) R 1 2 3 4 5 6 7 8 9 10 100 200 0 0 20 0 4 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 3";

#[test]
fn parse_stat_fields_handles_paren_in_comm() {
    let (comm, state, utime, stime, nice, threads, cpu, blkio) =
        parse_stat_fields(STAT_LINE).expect("must parse");
    assert_eq!(comm, "my prog (x)");
    assert_eq!(state, 'R');
    assert_eq!(utime, 100);
    assert_eq!(stime, 200);
    assert_eq!(nice, 0);
    assert_eq!(threads, 4);
    assert_eq!(cpu, 3);
    // Fixture tail is all zeros past index 36: blkio defaults to 0.
    assert_eq!(blkio, 0);
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
        cmdline: vec!["demo".into(), "--flag".into()],
        username: "root".into(),
        num_threads: 3,
        state: 'S',
        nice: 0,
        gids: (0, 0, 0),
        cpu_percent: 12.345,
        memory_percent: 0.5,
        rss: 1024,
        vms: 2048,
        mem_shared: 512,
        mem_text: 64,
        mem_lib: 32,
        mem_data: 128,
        mem_dirty: 8,
        utime: 10,
        stime: 5,
        iowait_ticks: 7,
        read_bytes: 100,
        write_bytes: 200,
        read_count: 10,
        read_rate: 0.0,
        write_rate: 0.0,
        write_count: 20,
        cpu_num: 1,
        time_since_update: 2.5,
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
    // Upstream parity: cmdline is an argv array, gids/memory_info/
    // cpu_times/io_counters carry the full sub-key sets.
    let cmd = obj.get("cmdline").and_then(|v| v.as_array()).expect("cmdline array");
    assert_eq!(cmd.len(), 2);
    assert!(obj.get("gids").and_then(|v| v.as_object()).is_some());
    let mem = obj
        .get("memory_info")
        .and_then(|v| v.as_object())
        .expect("memory_info");
    assert_eq!(mem.get("rss").and_then(|v| v.as_f64()), Some(1024.0));
    for k in ["shared", "text", "lib", "data", "dirty"] {
        assert!(mem.contains_key(k), "memory_info missing {k}");
    }
    let times = obj.get("cpu_times").and_then(|v| v.as_object()).expect("cpu_times");
    assert!(times.contains_key("iowait"), "cpu_times missing iowait");
    let io = obj.get("io_counters").and_then(|v| v.as_object()).expect("io_counters");
    for k in ["read_count", "write_count", "read_bytes", "write_bytes"] {
        assert!(io.contains_key(k), "io_counters missing {k}");
    }
}

#[test]
fn sample_all_second_tick_prunes_and_percents() {
    // Live /proc walk: must not panic; pids observed twice get percents.
    let mut prev = HashMap::new();
    let mut seen = HashMap::new();
    let now = std::time::Instant::now();
    let first = crate::plugins::processlist::sample_all(&mut prev, &mut seen, now);
    assert!(!first.is_empty());
    assert!(first.iter().all(|p| p.cpu_percent == 0.0));
    assert!(first.iter().all(|p| p.read_rate == 0.0 && p.write_rate == 0.0));
    let second = crate::plugins::processlist::sample_all(&mut prev, &mut seen, now);
    assert!(!second.is_empty());
    assert!(second.iter().all(|p| p.cpu_percent >= 0.0));
    assert!(second.iter().all(|p| p.read_rate >= 0.0 && p.write_rate >= 0.0));
}

#[test]
fn register_plugin_appears_in_stats() {
    let s = crate::core::stats::GlancesStats::new(1.0);
    register(&s);
    assert!(s.plugin_names().contains(&"processlist"));
}

#[test]
fn parse_status_text_collects_gids() {
    // Upstream gids contract: (real, effective, saved) triple.
    let text = "Name:\ttest\nState:\tS (sleeping)\nUid:\t1000\t1000\t1000\t1000\nGid:\t100\t200\t300\t400\n";
    assert_eq!(parse_status_text(text), Some(('S', 1000, (100, 200, 300))));
    // Missing Gid line falls back to uid triple.
    let text2 = "State:\tR (running)\nUid:\t0\t0\t0\t0\n";
    assert_eq!(parse_status_text(text2), Some(('R', 0, (0, 0, 0))));
}

#[test]
fn parse_statm_text_reads_all_seven_fields() {
    // statm order: size resident shared text lib data dirty (pages).
    let v = parse_statm_text("100 50 20 5 2 10 1", 4096).expect("must parse");
    assert_eq!(v, (409600, 204800, 81920, 20480, 8192, 40960, 4096));
    assert!(parse_statm_text("100 50", 4096).is_none());
}

#[test]
fn parse_io_text_reads_counts_as_syscr_syscw() {
    // Upstream io_counters: read_count=syscr, write_count=syscw.
    let text = "rchar: 100\nwchar: 200\nsyscr: 7\nsyscw: 9\nread_bytes: 1000\nwrite_bytes: 2000\ncancelled_write_bytes: 0\n";
    assert_eq!(parse_io_text(text), (1000, 2000, 7, 9));
}
