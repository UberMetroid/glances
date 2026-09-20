//! Tests for Linux /proc/meminfo parser.

use crate::platform::linux::proc_meminfo;

const NORMAL: &str = include_str!("../fixtures/proc/meminfo_normal.txt");

#[test]
fn parse_returns_bytes_not_kilobytes() {
    let m = proc_meminfo::parse(NORMAL).unwrap();
    // Fixture is in kB; fields must be kB * 1024 (psutil byte parity).
    assert_eq!(m.total, 16384000 * 1024);
    assert_eq!(m.free, 8192000 * 1024);
    assert_eq!(m.available, 12288000 * 1024);
    assert_eq!(m.buffers, 409600 * 1024);
    assert_eq!(m.cached, 2048000 * 1024);
    assert_eq!(m.swap_total, 4096000 * 1024);
    assert_eq!(m.swap_free, 4096000 * 1024);
    assert_eq!(m.shared, 50000 * 1024);
}

#[test]
fn used_is_psutil_formula_total_minus_free_buffers_cached() {
    // psutil: used = total - free - buffers - cached (NOT total - available).
    let m = proc_meminfo::MemInfo {
        total: 1000,
        free: 200,
        available: 600, // deliberately different from free+buffers+cached
        buffers: 50,
        cached: 100,
        ..Default::default()
    };
    assert_eq!(proc_meminfo::used(&m), 1000 - 200 - 50 - 100);
}

#[test]
fn percent_uses_available_base() {
    // psutil: percent = (total - available) / total * 100.
    let m = proc_meminfo::MemInfo {
        total: 1000,
        free: 200,
        available: 600,
        buffers: 50,
        cached: 100,
        ..Default::default()
    };
    assert_eq!(proc_meminfo::percent_used(&m), 40.0);
}

#[test]
fn percent_falls_back_to_free_buffers_cached_base() {
    let m = proc_meminfo::MemInfo {
        total: 1000,
        free: 200,
        available: 0,
        buffers: 50,
        cached: 100,
        ..Default::default()
    };
    assert_eq!(proc_meminfo::percent_used(&m), 65.0);
}

#[test]
fn free_is_raw_memfree_not_available() {
    // `free` must be MemFree even when MemAvailable is present —
    // psutil's vm.free is MemFree; available is a separate field.
    let m = proc_meminfo::MemInfo {
        total: 1000,
        free: 200,
        available: 600,
        buffers: 50,
        cached: 100,
        ..Default::default()
    };
    assert_eq!(proc_meminfo::free_mem(&m), 200);
}

#[test]
fn parse_tolerates_missing_keys() {
    let input = "MemTotal:    1000 kB\nSomeOtherKey:  999 kB\n";
    let m = proc_meminfo::parse(input).unwrap();
    assert_eq!(m.total, 1000 * 1024);
    assert_eq!(m.free, 0);
}

#[test]
fn parse_skips_malformed_values() {
    let input = "MemTotal: garbage kB\nMemFree: 200 kB\n";
    let m = proc_meminfo::parse(input).unwrap();
    assert_eq!(m.total, 0);
    assert_eq!(m.free, 200 * 1024);
}

#[test]
fn parse_handles_no_kb_suffix() {
    let input = "MemTotal: 1000\n";
    let m = proc_meminfo::parse(input).unwrap();
    // Bare numbers are still treated as kB (meminfo is always kB).
    assert_eq!(m.total, 1000 * 1024);
}

#[test]
fn read_or_error() {
    let r = proc_meminfo::read();
    if cfg!(target_os = "linux") { assert!(r.is_ok()); }
}

#[test]
fn parse_collects_active_and_inactive() {
    // Upstream mem contract: Active/Inactive are first-class fields.
    // `Active(anon)`-style subkeys must NOT leak into them.
    let m = proc_meminfo::parse(NORMAL).unwrap();
    assert_eq!(m.active, 4096000 * 1024);
    assert_eq!(m.inactive, 2048000 * 1024);
}

#[test]
fn parse_arcstats_reads_size_and_c_min() {
    // Upstream zfs_stats: two header lines, then `name _ value` rows.
    let text = "kstat+zfs:0:arcstats:magic\nname type data\nsize 4 1073741824\nc_min 4 67108864\nc_max 4 2147483648\n";
    assert_eq!(proc_meminfo::parse_arcstats(text), Some((1073741824, 67108864)));
    assert_eq!(proc_meminfo::parse_arcstats("a\nb\nc_max 4 1\n"), None);
}
