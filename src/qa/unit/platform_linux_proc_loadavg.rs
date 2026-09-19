//! Tests for Linux /proc/loadavg parser.

use crate::platform::linux::proc_loadavg;

const NORMAL: &str = "0.50 0.40 0.30 1/1234 5678\n";

#[test]
fn parse_normal() {
    let l = proc_loadavg::parse(NORMAL).unwrap();
    assert_eq!(l.load1, 0.50);
    assert_eq!(l.load5, 0.40);
    assert_eq!(l.load15, 0.30);
    assert_eq!(l.running, 1);
    assert_eq!(l.total, 1234);
    assert_eq!(l.last_pid, 5678);
}

#[test]
fn parse_three_field_only() {
    // Some kernels omit the last_pid field.
    let l = proc_loadavg::parse("1.5 2.0 3.0 5/678").unwrap();
    assert_eq!(l.load1, 1.5);
    assert_eq!(l.running, 5);
    assert_eq!(l.total, 678);
    assert_eq!(l.last_pid, 0);
}

#[test]
fn parse_rejects_empty() {
    assert!(proc_loadavg::parse("").is_err());
    assert!(proc_loadavg::parse("\n").is_err());
}

#[test]
fn parse_rejects_bad_numbers() {
    assert!(proc_loadavg::parse("notanumber 0.5 1.0 1/2 3").is_err());
}

#[test]
fn parse_handles_huge_load() {
    let l = proc_loadavg::parse("99999.99 88888.88 77777.77 100/1000 99999").unwrap();
    assert_eq!(l.load1, 99999.99);
}

#[test]
fn read_or_error() {
    let r = proc_loadavg::read();
    if cfg!(target_os = "linux") { assert!(r.is_ok()); }
}
