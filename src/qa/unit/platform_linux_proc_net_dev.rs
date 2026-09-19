//! Tests for Linux /proc/net/dev parser.

use crate::platform::linux::proc_net_dev;

const NORMAL: &str = include_str!("../fixtures/proc/net_dev_normal.txt");

#[test]
fn parse_normal_three_interfaces() {
    let v = proc_net_dev::parse(NORMAL).unwrap();
    assert_eq!(v.len(), 3);
    let names: Vec<&str> = v.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["lo", "eth0", "wlan0"]);
}

#[test]
fn parse_normal_eth0_stats() {
    let v = proc_net_dev::parse(NORMAL).unwrap();
    let eth0 = &v[1].1;
    assert_eq!(eth0.rx_bytes, 5000000);
    assert_eq!(eth0.rx_packets, 30000);
    assert_eq!(eth0.tx_bytes, 2000000);
    assert_eq!(eth0.tx_packets, 20000);
    assert_eq!(eth0.rx_errors, 0);
    assert_eq!(eth0.tx_errors, 0);
}

#[test]
fn parse_normal_wlan0_has_one_error() {
    let v = proc_net_dev::parse(NORMAL).unwrap();
    let wlan0 = &v[2].1;
    assert_eq!(wlan0.rx_errors, 1);
}

#[test]
fn parse_rejects_truncated_lines() {
    // Fewer than 8 numbers — should be skipped, not panic.
    let input = "Inter-|   Receive\n face\neth0: 1 2\n";
    let v = proc_net_dev::parse(input).unwrap();
    assert!(v.is_empty());
}

#[test]
fn parse_rejects_8_to_11_field_lines() {
    // Regression: the bounds check used nums.len() < 8 but indexed up
    // to nums[11] — an 8-11 field line panicked. Must skip, not panic.
    for n in 8..12 {
        let nums: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
        let input = format!("Inter-|   Receive\n face\neth0: {}\n", nums.join(" "));
        let v = proc_net_dev::parse(&input).unwrap();
        assert!(v.is_empty(), "{} fields must be skipped", n);
    }
}

#[test]
fn parse_handles_extra_columns() {
    // Newer kernels add 4 more columns (discards, flush). We just ignore them.
    let input = "Inter-|   Receive                                                |  Transmit\n face\neth0: 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18\n";
    let v = proc_net_dev::parse(input).unwrap();
    assert_eq!(v.len(), 1);
    let eth0 = &v[0].1;
    assert_eq!(eth0.rx_bytes, 1);
    assert_eq!(eth0.tx_bytes, 9);
}

#[test]
fn read_or_error() {
    let r = proc_net_dev::read();
    if cfg!(target_os = "linux") { assert!(r.is_ok()); }
}
