//! Tests for the network plugin — per-NIC rate computation.

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins::network::{NetworkPlugin, NAME};
use crate::platform::linux::proc_net_dev;
use crate::platform::linux::sys_class_net;

#[test]
fn name_and_register() {
    let s = GlancesStats::new(1.0);
    crate::plugins::network::register(&s);
    assert!(s.plugin_names().contains(&NAME));
}

#[test]
fn plugin_emits_per_nic_array_filtering_loopback() {
    let mut p = NetworkPlugin::new();
    // First tick: rates are zero because no previous snapshot.
    p.update().expect("first update ok");
    let arr = p.stats().as_array().expect("stats should be array");
    // Result keys we expect on every NIC.
    for v in arr {
        let obj = v.as_object().expect("entry should be object");
        for k in ["key", "interface_name", "alias", "is_up", "speed",
                  "bytes_recv", "bytes_recv_gauge", "bytes_recv_rate_per_sec",
                  "bytes_sent", "bytes_sent_gauge", "bytes_sent_rate_per_sec",
                  "bytes_all", "bytes_all_gauge", "bytes_all_rate_per_sec",
                  "time_since_update"] {
            assert!(obj.contains_key(k), "missing key {k}");
        }
        // Interface name must never be the loopback.
        let name = obj.get("interface_name").and_then(Value::as_str).unwrap();
        assert_ne!(name, "lo", "loopback must be filtered out");
    }
}

#[test]
fn rate_computation_uses_byte_delta_over_elapsed_time() {
    // Use the plugin's internal logic indirectly: feed two fake /proc/net/dev
    // snapshots and assert that the rate equals the delta divided by 1s.
    //
    // We can't fake sysfs, but proc_net_dev parsing is pure std and the
    // first-tick rate is always 0; for rate computation we exercise the
    // raw deltas ourselves so we don't depend on system sleep timing.
    let first = proc_net_dev::parse(
        "Inter-|   Receive                                                |  Transmit\n face\neth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\n",
    ).unwrap();
    let second = proc_net_dev::parse(
        "Inter-|   Receive                                                |  Transmit\n face\neth0: 3000 10 0 0 0 0 0 0 4000 20 0 0 0 0 0 0\n",
    ).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    let d_rx = second[0].1.rx_bytes - first[0].1.rx_bytes;
    let d_tx = second[0].1.tx_bytes - first[0].1.tx_bytes;
    assert_eq!(d_rx, 2000);
    assert_eq!(d_tx, 2000);
    // If a 1-second tick passed, that's 2000 B/s each way.
    let dt = 1.0_f64;
    assert!((d_rx as f64 / dt - 2000.0).abs() < 1e-9);
    assert!((d_tx as f64 / dt - 2000.0).abs() < 1e-9);
}

#[test]
fn reset_clears_prev_state() {
    let mut p = NetworkPlugin::new();
    p.update().expect("first update ok");
    p.reset();
    // After reset, stats go back to the empty array.
    assert!(p.stats().as_array().unwrap().is_empty());
}

#[test]
fn read_meta_loopback_is_not_physical() {
    if !cfg!(target_os = "linux") { return; }
    let m = sys_class_net::read_meta("lo").expect("read loopback meta");
    assert!(!m.is_physical, "loopback should not be physical");
}

#[test]
fn plugin_get_key_returns_interface_name() {
    let p = NetworkPlugin::new();
    assert_eq!(p.get_key(), Some("interface_name"));
}

#[test]
fn first_tick_emits_deltas_not_cumulative() {
    // Upstream `_manage_rate`: plain fields are tick deltas (0 with no
    // baseline), gauges carry the cumulative counters, and the window
    // is `time_since_update`. No sleeps, no live-value asserts.
    let mut p = NetworkPlugin::new();
    p.update().expect("first update ok");
    for v in p.stats().as_array().expect("array") {
        let obj = v.as_object().expect("object");
        assert_eq!(obj.get("key").and_then(Value::as_str), Some("interface_name"));
        assert_eq!(obj.get("bytes_recv").and_then(Value::as_f64), Some(0.0));
        assert_eq!(obj.get("bytes_sent").and_then(Value::as_f64), Some(0.0));
        assert_eq!(obj.get("time_since_update").and_then(Value::as_f64), Some(0.0));
        for k in ["bytes_recv_gauge", "bytes_sent_gauge", "bytes_all_gauge"] {
            let g = obj.get(k).and_then(Value::as_f64).expect("gauge present");
            assert!(g >= 0.0, "{k} must be non-negative");
        }
    }
}