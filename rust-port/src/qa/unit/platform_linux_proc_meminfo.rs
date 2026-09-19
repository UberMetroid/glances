//! Tests for Linux /proc/meminfo parser.

use crate::platform::linux::proc_meminfo;

const NORMAL: &str = include_str!("../fixtures/proc/meminfo_normal.txt");

#[test]
fn parse_normal_basic_fields() {
    let m = proc_meminfo::parse(NORMAL).unwrap();
    assert_eq!(m.total, 16384000);
    assert_eq!(m.free, 8192000);
    assert_eq!(m.available, 12288000);
    assert_eq!(m.buffers, 409600);
    assert_eq!(m.cached, 2048000);
    assert_eq!(m.swap_total, 4096000);
    assert_eq!(m.swap_free, 4096000);
    assert_eq!(m.shared, 50000);
}

#[test]
fn used_uses_available_when_present() {
    let m = proc_meminfo::MemInfo {
        total: 16384000,
        free: 8192000,
        available: 12288000,
        ..Default::default()
    };
    assert_eq!(proc_meminfo::used(&m), 16384000 - 12288000);
}

#[test]
fn used_falls_back_to_total_minus_free_minus_buffers_minus_cached() {
    let m = proc_meminfo::MemInfo {
        total: 1000,
        free: 200,
        available: 0, // unavailable on this old kernel
        buffers: 50,
        cached: 100,
        ..Default::default()
    };
    assert_eq!(proc_meminfo::used(&m), 1000 - 200 - 50 - 100);
}

#[test]
fn free_returns_available_when_present() {
    let m = proc_meminfo::MemInfo {
        total: 1000,
        free: 200,
        available: 600,
        buffers: 50,
        cached: 100,
        ..Default::default()
    };
    assert_eq!(proc_meminfo::free_mem(&m), 600);
}

#[test]
fn free_falls_back_to_free_plus_buffers_plus_cached() {
    let m = proc_meminfo::MemInfo {
        total: 1000,
        free: 200,
        available: 0,
        buffers: 50,
        cached: 100,
        ..Default::default()
    };
    assert_eq!(proc_meminfo::free_mem(&m), 200 + 50 + 100);
}

#[test]
fn parse_tolerates_missing_keys() {
    let input = "MemTotal:    1000 kB\nSomeOtherKey:  999 kB\n";
    let m = proc_meminfo::parse(input).unwrap();
    assert_eq!(m.total, 1000);
    assert_eq!(m.free, 0);
}

#[test]
fn parse_skips_malformed_values() {
    let input = "MemTotal: garbage kB\nMemFree: 200 kB\n";
    let m = proc_meminfo::parse(input).unwrap();
    assert_eq!(m.total, 0);
    assert_eq!(m.free, 200);
}

#[test]
fn parse_handles_no_kb_suffix() {
    let input = "MemTotal: 1000\n";
    let m = proc_meminfo::parse(input).unwrap();
    assert_eq!(m.total, 1000);
}

#[test]
fn read_or_error() {
    let r = proc_meminfo::read();
    if cfg!(target_os = "linux") { assert!(r.is_ok()); }
}
