//! Tests for the ports plugin — TCP/UDP socket listing.

use crate::plugins::ports::{
    collect, decode_ipv4, decode_ipv6, parse, split_addr, tcp_state_name, NetRow,
};

// Minimal 12-column /proc/net/tcp fixture — the kernel emits extra
// fields (ref, pointer, drops, ...) on modern systems, but the legacy
// 12 columns sl..inode are what our parser cares about.
const PROC_NET_TCP_FIXTURE: &str = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:A6D1 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 100709138 2
   1: 0100007F:0050 0100007F:0051 01 00000000:00000000 00:00000000 00000000  1000        0 100709139 2
   2: 00000000:0050 0100007F:1234 06 00000000:00000000 00:00000000 00000000  1000        0 100709140 2
";

#[test]
fn decode_ipv4_round_trips_loopback() {
    // 127.0.0.1 in little-endian byte order.
    assert_eq!(decode_ipv4("0100007F").as_deref(), Some("127.0.0.1"));
    // 0.0.0.0
    assert_eq!(decode_ipv4("00000000").as_deref(), Some("0.0.0.0"));
    // 192.168.0.1 → 01 00 A8 C0
    assert_eq!(decode_ipv4("0100A8C0").as_deref(), Some("192.168.0.1"));
}

#[test]
fn decode_ipv4_rejects_wrong_length() {
    assert!(decode_ipv4("").is_none());
    assert!(decode_ipv4("01").is_none());
    assert!(decode_ipv4("0100007F00").is_none());
}

#[test]
fn decode_ipv6_rejects_wrong_length() {
    assert!(decode_ipv6("").is_none());
    assert!(decode_ipv6("00").is_none());
    // Wrong length (8 chars) — IPv4 length.
    assert!(decode_ipv6("0100007F").is_none());
}

#[test]
fn decode_ipv6_reverses_each_32bit_word() {
    // Regression: the kernel stores each 32-bit word little-endian.
    // ::1 arrives as ...01000000 (last word) → "0:0:0:0:0:0:0:1".
    assert_eq!(
        decode_ipv6("00000000000000000000000001000000").as_deref(),
        Some("0:0:0:0:0:0:0:1"),
    );
    // fe80::1 → first word FE80 byte-swapped in place.
    let v = decode_ipv6("000080FE000000000000000001000000").unwrap();
    assert_eq!(v, "fe80:0:0:0:0:0:0:1");
}

#[test]
fn tcp_state_name_maps_known_hex() {
    assert_eq!(tcp_state_name("01"), "ESTABLISHED");
    assert_eq!(tcp_state_name("0A"), "LISTEN");
    assert_eq!(tcp_state_name("06"), "TIME_WAIT");
    assert_eq!(tcp_state_name("08"), "CLOSE_WAIT");
    assert_eq!(tcp_state_name("FF"), "UNKNOWN");
}

#[test]
fn split_addr_decodes_ip_and_port() {
    let (ip, port) = split_addr("0100007F:A6D1");
    assert_eq!(ip, "127.0.0.1");
    // 0xA6D1 = 42705
    assert_eq!(port, 42705);
    let (any_ip, zero_port) = split_addr("00000000:0000");
    assert_eq!(any_ip, "0.0.0.0");
    assert_eq!(zero_port, 0);
}

#[test]
fn parse_handles_header_and_short_lines() {
    let text = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 100709138 2 0000000000000000 100 0 0 10 0
eth0: not enough columns
";
    let rows = parse(text, "tcp").unwrap();
    assert_eq!(rows.len(), 1, "short line should be skipped");
    assert_eq!(rows[0].st, "0A");
    assert_eq!(rows[0].local_address, "0100007F:0050");
    assert_eq!(rows[0].family, "tcp");
}

#[test]
fn parse_extracts_all_twelve_columns() {
    let rows = parse(PROC_NET_TCP_FIXTURE, "tcp").unwrap();
    assert_eq!(rows.len(), 3);
    let listen: &NetRow = &rows[0];
    assert_eq!(listen.sl, "0");
    assert_eq!(listen.inode, "100709138");
    assert_eq!(listen.st, "0A");
    let established: &NetRow = &rows[1];
    assert_eq!(established.st, "01");
    let time_wait: &NetRow = &rows[2];
    assert_eq!(time_wait.st, "06");
}

#[test]
fn parse_accepts_old_split_column_format() {
    // Very old kernels emit tx/rx and tr/tm as separate columns
    // (12 tokens, no "X:Y" pairs).
    let text = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0050 00000000:0000 0A 00000000 00000000 00 00000000 00000000 1000 0 12345
";
    let rows = parse(text, "tcp").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tx_queue, "00000000");
    assert_eq!(rows[0].inode, "12345");
}

#[test]
fn parse_skips_short_non_combined_line_without_panic() {
    // Regression: an 11-token line without combined "X:Y" queues
    // indexed parts[11] and panicked. It must be skipped instead.
    let text = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   1: 0100007F:0050 00000000:0000 0A 00000000 00000000 00 00000000 00000000 1000 0
";
    let rows = parse(text, "tcp").unwrap();
    assert!(rows.is_empty());
}

#[test]
fn collect_swallows_missing_paths() {
    // Pass a path that doesn't exist; collect() must not panic.
    let paths: [(&'static str, &'static str); 1] = [("/no/such/path", "tcp")];
    let rows = collect(&paths);
    assert!(rows.is_empty());
}

#[test]
fn register_plugin_appears_in_stats() {
    let s = crate::core::stats::GlancesStats::new(1.0);
    crate::plugins::ports::register(&s);
    assert!(s.plugin_names().contains(&"ports"));
}