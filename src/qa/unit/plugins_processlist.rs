//! Tests for the processlist plugin — stat/status/statm/io parsers,
//! row rendering, and the live two-tick sampler.

use std::collections::HashMap;

use crate::plugins::processlist::{
    build_user_map, parse_io_text, parse_stat_fields, parse_statm_text, parse_status_text,
    read_total_cpu, register, sample_to_value, status_name, ProcSample,
};

// Stat tail layout after ")": state ppid pgrp sess tty tpgid flags
// minflt cminflt majflt cmajflt utime stime cutime cstime prio nice
// threads, then the trailing block ending in cpu 2 / blkio 42.
const STAT_LINE: &str = "987 (worker (pool-2)) S 1 987 987 0 -1 1077936384 500 0 10 0 300 150 0 0 20 5 8 0 12345 1000000 500 1000000 0 0 0 0 0 0 0 0 0 0 0 0 0 2 0 0 42";

#[test]
fn parse_stat_fields_handles_paren_in_comm() {
    let (comm, state, utime, stime, nice, threads, cpu, blkio) =
        parse_stat_fields(STAT_LINE).expect("must parse");
    assert_eq!(comm, "worker (pool-2)");
    assert_eq!(state, 'S');
    assert_eq!(utime, 300);
    assert_eq!(stime, 150);
    assert_eq!(nice, 5);
    assert_eq!(threads, 8);
    assert_eq!(cpu, 2);
    assert_eq!(blkio, 42);
}

#[test]
fn parse_stat_fields_rejects_short_lines() {
    assert!(parse_stat_fields("7 (x) S 1").is_none());
    assert!(parse_stat_fields("987 (unclosed S 1 2 3").is_none());
    assert!(parse_stat_fields("garbage").is_none());
    assert!(parse_stat_fields("").is_none());
}

#[test]
fn status_name_maps_every_state() {
    for (c, want) in [
        ('R', "running"),
        ('S', "sleeping"),
        ('D', "disk-sleep"),
        ('T', "stopped"),
        ('t', "stopped"),
        ('Z', "zombie"),
        ('X', "zombie"),
        ('x', "zombie"),
        ('I', "idle"),
        ('Q', "unknown"),
    ] {
        assert_eq!(status_name(c), want, "state {c}");
    }
}

#[test]
fn user_map_and_total_cpu_read_live() {
    assert!(!build_user_map().is_empty(), "/etc/passwd must yield users");
    assert!(read_total_cpu() > 0, "/proc/stat aggregate must be positive");
}

fn sample() -> ProcSample {
    ProcSample {
        pid: 7,
        name: "svc".into(),
        cmdline: vec!["svc".into(), "--serve".into(), "8080".into()],
        username: "daemon".into(),
        num_threads: 5,
        state: 'D',
        nice: -5,
        gids: (50, 60, 70),
        cpu_percent: 12.346,
        memory_percent: 0.126,
        rss: 4096,
        vms: 8192,
        mem_shared: 1024,
        mem_text: 256,
        mem_lib: 128,
        mem_data: 512,
        mem_dirty: 16,
        utime: 30,
        stime: 12,
        iowait_ticks: 3,
        read_bytes: 5000,
        write_bytes: 7000,
        read_count: 11,
        write_count: 13,
        read_rate: 1.5,
        write_rate: 2.5,
        cpu_num: 2,
        time_since_update: 1.25,
    }
}

#[test]
fn sample_to_value_carries_all_keys() {
    let v = sample_to_value(&sample());
    let obj = v.as_object().expect("object");
    for k in [
        "pid",
        "key",
        "time_since_update",
        "name",
        "cmdline",
        "username",
        "gids",
        "num_threads",
        "cpu_percent",
        "memory_percent",
        "memory_info",
        "status",
        "nice",
        "cpu_times",
        "io_counters",
        "disk_read_rate_per_sec",
        "disk_write_rate_per_sec",
        "cpu_num",
    ] {
        assert!(obj.contains_key(k), "missing key {k}");
    }
    assert_eq!(obj.get("status").and_then(|v| v.as_str()), Some("disk-sleep"));
    // Percentages round to two decimals.
    assert_eq!(obj.get("cpu_percent").and_then(|v| v.as_f64()), Some(12.35));
    assert_eq!(obj.get("memory_percent").and_then(|v| v.as_f64()), Some(0.13));
    // cmdline is an argv array, not a joined string.
    let cmd = obj.get("cmdline").and_then(|v| v.as_array()).expect("cmdline array");
    assert_eq!(cmd.len(), 3);
    let gids = obj.get("gids").and_then(|v| v.as_object()).expect("gids");
    assert_eq!(gids.get("saved").and_then(|v| v.as_f64()), Some(70.0));
    let mem = obj.get("memory_info").and_then(|v| v.as_object()).expect("memory_info");
    assert_eq!(mem.get("rss").and_then(|v| v.as_f64()), Some(4096.0));
    for k in ["shared", "text", "lib", "data", "dirty"] {
        assert!(mem.contains_key(k), "memory_info missing {k}");
    }
    let times = obj.get("cpu_times").and_then(|v| v.as_object()).expect("cpu_times");
    assert_eq!(times.get("iowait").and_then(|v| v.as_f64()), Some(3.0));
    let io = obj.get("io_counters").and_then(|v| v.as_object()).expect("io_counters");
    assert_eq!(io.get("read_count").and_then(|v| v.as_f64()), Some(11.0));
    assert_eq!(io.get("write_count").and_then(|v| v.as_f64()), Some(13.0));
    assert_eq!(obj.get("nice").and_then(|v| v.as_i64()), Some(-5));
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
    assert!(first.windows(2).all(|w| w[0].pid <= w[1].pid), "sorted by pid");
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
    let text = "Name:\tworker\nState:\tD (disk sleep)\nUid:\t1001\t1001\t1001\t1001\nGid:\t50\t60\t70\t80\n";
    assert_eq!(parse_status_text(text), Some(('D', 1001, (50, 60, 70))));
    // Missing Gid line falls back to the uid triple.
    let text2 = "State:\tT (stopped)\nUid:\t5\t5\t5\t5\n";
    assert_eq!(parse_status_text(text2), Some(('T', 5, (5, 5, 5))));
    // No uid at all is unusable.
    assert!(parse_status_text("State:\tR (running)\n").is_none());
}

#[test]
fn parse_statm_text_reads_all_seven_fields() {
    // statm order: size resident shared text lib data dirty (pages).
    let v = parse_statm_text("200 100 40 8 4 20 2", 4096).expect("must parse");
    assert_eq!(v, (819200, 409600, 163840, 32768, 16384, 81920, 8192));
    assert!(parse_statm_text("200 100", 4096).is_none());
}

#[test]
fn parse_io_text_reads_counts_as_syscr_syscw() {
    let text =
        "rchar: 5\nwchar: 6\nsyscr: 11\nsyscw: 13\nread_bytes: 5000\nwrite_bytes: 7000\n";
    assert_eq!(parse_io_text(text), (5000, 7000, 11, 13));
    assert_eq!(parse_io_text(""), (0, 0, 0, 0));
}
