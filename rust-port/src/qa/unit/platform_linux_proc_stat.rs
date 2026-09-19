//! Tests for Linux /proc/stat parser.

use crate::platform::linux::proc_stat;
use std::fs;

const NORMAL: &str = include_str!("../fixtures/proc/stat_normal.txt");

#[test]
fn parse_normal_total() {
    let p = proc_stat::parse(NORMAL).unwrap();
    assert_eq!(p.total.user, 4705);
    assert_eq!(p.total.idle, 369998);
    assert_eq!(p.total.guest, 0);
    assert_eq!(p.ctxt, 9876543);
    assert_eq!(p.btime, 1700000000);
    assert_eq!(p.processes, 12345);
}

#[test]
fn parse_normal_per_cpu() {
    let p = proc_stat::parse(NORMAL).unwrap();
    assert_eq!(p.per_cpu.len(), 4);
    assert_eq!(p.per_cpu[0].user, 1144);
    assert_eq!(p.per_cpu[3].system, 398);
}

#[test]
fn cpu_times_busy_and_total() {
    let t = proc_stat::CpuTimes {
        user: 100, nice: 0, system: 50, idle: 200,
        iowait: 25, irq: 0, softirq: 0, steal: 0,
        guest: 0, guest_nice: 0,
    };
    assert_eq!(t.busy(), 175);
    assert_eq!(t.total(), 375);
}

#[test]
fn read_returns_or_errors_gracefully() {
    // /proc/stat should always exist on Linux. On other OSes it errors.
    let r = proc_stat::read();
    if cfg!(target_os = "linux") {
        assert!(r.is_ok(), "/proc/stat should be readable on Linux");
    } else {
        assert!(r.is_err());
    }
}

#[test]
fn parse_handles_empty_lines() {
    let input = "cpu 100 0 50 200 0 0 0 0 0 0\n\nctxt 5\n";
    let p = proc_stat::parse(input).unwrap();
    assert_eq!(p.total.idle, 200);
    assert_eq!(p.ctxt, 5);
}

#[test]
fn parse_handles_truncated_lines() {
    // Malformed input — should not panic, just skip what it can't parse.
    let input = "cpu 100 0 50\nctxt notanumber\nbtime\n";
    let p = proc_stat::parse(input).unwrap();
    assert_eq!(p.total.user, 100);
    assert_eq!(p.ctxt, 0);
    assert_eq!(p.btime, 0);
}

#[test]
fn parse_ignores_unknown_lines() {
    let input = "cpu 100 0 50 200 0 0 0 0 0 0\nweirdline whatever\nctxt 7\n";
    let p = proc_stat::parse(input).unwrap();
    assert_eq!(p.ctxt, 7);
    assert_eq!(p.total.user, 100);
}

#[test]
fn fixture_loads_and_matches() {
    // Sanity: make sure the fixture itself is parseable.
    let txt = fs::read_to_string("src/qa/fixtures/proc/stat_normal.txt").unwrap();
    let _ = proc_stat::parse(&txt).unwrap();
}
