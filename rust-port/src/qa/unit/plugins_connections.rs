//! Tests for the connections plugin — TCP state counts.

use crate::core::plugin::Plugin;
use crate::plugins::connections::{tally_rows, empty_stats};
use crate::plugins::ports::parse;

const FIXTURE: &str = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 1 2 0000000000000000 100 0 0 10 0
   1: 0100007F:0051 0100007F:0052 01 00000000:00000000 00:00000000 00000000  1000        0 2 2 0000000000000000 100 0 0 10 0
   2: 0100007F:0053 0100007F:0054 01 00000000:00000000 00:00000000 00000000  1000        0 3 2 0000000000000000 100 0 0 10 0
   3: 00000000:0055 0100007F:1234 06 00000000:00000000 00:00000000 00000000  1000        0 4 2 0000000000000000 100 0 0 10 0
   4: 00000000:0056 0100007F:1235 08 00000000:00000000 00:00000000 00000000  1000        0 5 2 0000000000000000 100 0 0 10 0
   5: 00000000:0057 0100007F:1236 FF 00000000:00000000 00:00000000 00000000  1000        0 6 2 0000000000000000 100 0 0 10 0
";

#[test]
fn empty_stats_has_canonical_state_keys() {
    let s = empty_stats();
    let obj = s.as_object().expect("stats must be object");
    for key in [
        "ESTABLISHED", "LISTEN", "TIME_WAIT", "CLOSE_WAIT",
        "SYN_SENT", "SYN_RECV", "FIN_WAIT1", "FIN_WAIT2",
        "CLOSE", "LAST_ACK", "CLOSING", "NEW_SYN_RECV", "UNKNOWN",
    ] {
        assert!(obj.contains_key(key), "missing canonical key {key}");
    }
}

#[test]
fn tally_counts_listen_and_established_correctly() {
    let rows = parse(FIXTURE, "tcp").unwrap();
    let counts = tally_rows(&rows);
    assert_eq!(counts.get("LISTEN").copied().unwrap_or(0), 1);
    assert_eq!(counts.get("ESTABLISHED").copied().unwrap_or(0), 2);
    assert_eq!(counts.get("TIME_WAIT").copied().unwrap_or(0), 1);
    assert_eq!(counts.get("CLOSE_WAIT").copied().unwrap_or(0), 1);
    assert_eq!(counts.get("UNKNOWN").copied().unwrap_or(0), 1);
    // Unseen states stay at zero.
    assert_eq!(counts.get("SYN_SENT").copied().unwrap_or(0), 0);
    assert_eq!(counts.get("FIN_WAIT2").copied().unwrap_or(0), 0);
}

#[test]
fn tally_ignores_udp_rows() {
    let rows = parse(FIXTURE, "tcp").unwrap();
    // Now construct a synthetic UDP-shaped row (we can't read /proc/net/udp
    // here, but the parser accepts any 12-column line).
    let udp_text = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0035 00000000:0000 07 00000000:00000000 00:00000000 00000000  193        0 9 2 0000000000000000 100 0 0 10 0
";
    let mut rows = rows;
    rows.extend(parse(udp_text, "udp").unwrap());
    let counts = tally_rows(&rows);
    // UDP must not inflate any TCP-state bucket.
    assert_eq!(counts.get("LISTEN").copied().unwrap_or(0), 1);
    assert_eq!(counts.get("ESTABLISHED").copied().unwrap_or(0), 2);
    assert_eq!(counts.get("CLOSE").copied().unwrap_or(0), 0);
}

#[test]
fn tally_with_no_rows_returns_zeros() {
    let counts = tally_rows(&[]);
    // Every canonical state is present, all zero.
    assert_eq!(counts.get("LISTEN").copied().unwrap_or(0), 0);
    assert_eq!(counts.get("ESTABLISHED").copied().unwrap_or(0), 0);
    assert_eq!(counts.get("UNKNOWN").copied().unwrap_or(0), 0);
}

#[test]
fn register_plugin_appears_in_stats() {
    let s = crate::core::stats::GlancesStats::new(1.0);
    crate::plugins::connections::register(&s);
    assert!(s.plugin_names().contains(&"connections"));
}

#[test]
fn plugin_emits_object_stats_after_update() {
    let mut p = crate::plugins::connections::ConnectionsPlugin::new();
    p.update().expect("update ok");
    let obj = p.stats().as_object().expect("connections stats is object");
    assert!(obj.contains_key("LISTEN"));
    assert!(obj.contains_key("ESTABLISHED"));
    // nf_conntrack fields are nullable when the proc entry isn't readable.
    assert!(obj.contains_key("nf_conntrack_count"));
    assert!(obj.contains_key("nf_conntrack_max"));
}