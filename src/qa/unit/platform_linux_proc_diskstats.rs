//! Tests for Linux /proc/diskstats parser.

use crate::platform::linux::proc_diskstats;

const NORMAL: &str = include_str!("../fixtures/proc/diskstats_normal.txt");

#[test]
fn parse_normal_filters_ram_and_loop() {
    let v = proc_diskstats::parse(NORMAL).unwrap();
    let names: Vec<&str> = v.iter().map(|d| d.name.as_str()).collect();
    // ram0 and loop0 are filtered; sda, sda1, nvme0n1 are kept (partitions
    // are also kept because we don't distinguish them yet).
    assert!(names.contains(&"sda"));
    assert!(names.contains(&"nvme0n1"));
    assert!(!names.contains(&"ram0"));
    assert!(!names.contains(&"loop0"));
}

#[test]
fn parse_normal_sda_stats() {
    let v = proc_diskstats::parse(NORMAL).unwrap();
    let sda = v.iter().find(|d| d.name == "sda").unwrap();
    assert_eq!(sda.reads_completed, 1000);
    assert_eq!(sda.sectors_read, 2000);
    assert_eq!(sda.writes_completed, 500);
    assert_eq!(sda.sectors_written, 1000);
}

#[test]
fn read_bytes_uses_512_sector_size() {
    let d = proc_diskstats::DiskStats {
        name: "x".into(),
        sectors_read: 100,
        sectors_written: 50,
        ..Default::default()
    };
    assert_eq!(proc_diskstats::read_bytes(&d), 51200);
    assert_eq!(proc_diskstats::write_bytes(&d), 25600);
}

#[test]
fn parse_skips_truncated_lines() {
    let v = proc_diskstats::parse("   8 0 sda 100 0 200 50 500\n").unwrap();
    assert!(v.is_empty());
}

#[test]
fn parse_handles_extra_discard_columns() {
    // 14+4 = 18 columns. Make sure it still parses.
    let line = "   8       0 sda 1 0 2 3 4 0 5 6 0 7 8 0 9 10 11 12";
    let v = proc_diskstats::parse(line).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].reads_completed, 1);
    assert_eq!(v[0].sectors_read, 2);
}

#[test]
fn read_or_error() {
    let r = proc_diskstats::read();
    if cfg!(target_os = "linux") { assert!(r.is_ok()); }
}
