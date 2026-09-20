//! Unit tests for the StatsD UDP exporter.

use std::collections::{BTreeMap, HashMap};
use std::net::UdpSocket;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::statsd;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
}

/// Bind a UDP socket on an ephemeral port and hand back (port, receiver)
/// of datagrams received within a short window.
fn udp_capture() -> (u16, mpsc::Receiver<Vec<u8>>) {
    let sock = UdpSocket::bind("127.0.0.1:0").expect("bind udp");
    let port = sock.local_addr().unwrap().port();
    sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match sock.recv(&mut buf) {
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() { break; }
                }
                Err(_) => break,
            }
        }
    });
    (port, rx)
}

fn drain(rx: &mpsc::Receiver<Vec<u8>>) -> String {
    let mut packets = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while let Ok(b) = rx.recv_timeout(Duration::from_millis(400)) {
        packets.push(String::from_utf8(b).unwrap());
        if std::time::Instant::now() >= deadline { break; }
    }
    packets.join("")
}

#[test]
fn gauge_for_plain_numeric_key() {
    let (port, rx) = udp_capture();
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(12.5))]))]);
    let cfg = statsd::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: None,
    };
    statsd::write(&flat(&snap), &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(2)).expect("packet");
    let pkt = String::from_utf8(bytes).unwrap();
    assert!(pkt.starts_with("glances.cpu.total:12.5|g"));
    assert!(pkt.ends_with('\n'));
}

#[test]
fn counter_for_total_and_count_suffixes() {
    let (port, rx) = udp_capture();
    let snap = obj(&[(
        "net",
        obj(&[
            ("rx_total", Value::Int(1024)),
            ("errors_count", Value::Uint(3)),
            ("rx", Value::Int(5)),
        ]),
    )]);
    let cfg = statsd::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: None,
    };
    statsd::write(&flat(&snap), &cfg).expect("write");
    let joined = drain(&rx);
    assert!(joined.contains("glances.net.rx_total:1024|c"), "missing rx_total: {}", joined);
    assert!(joined.contains("glances.net.errors_count:3|c"));
    assert!(joined.contains("glances.net.rx:5|g"));
}

#[test]
fn bool_becomes_0_or_1_gauge() {
    // Bools are state (gauge 1/0), not events — a counter would
    // double-count on every flush interval.
    let (port, rx) = udp_capture();
    let snap = obj(&[("now", obj(&[("up", Value::Bool(true)), ("down", Value::Bool(false))]))]);
    let cfg = statsd::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: None,
    };
    statsd::write(&flat(&snap), &cfg).expect("write");
    let joined = drain(&rx);
    assert!(joined.contains("glances.now.up:1|g"), "got: {}", joined);
    assert!(joined.contains("glances.now.down:0|g"), "got: {}", joined);
    assert!(!joined.contains("|c"));
}

#[test]
fn nan_and_inf_are_dropped() {
    let (port, rx) = udp_capture();
    let snap = obj(&[(
        "cpu",
        obj(&[
            ("bad_nan", Value::Float(f64::NAN)),
            ("good", Value::Int(7)),
        ]),
    )]);
    let cfg = statsd::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: None,
    };
    statsd::write(&flat(&snap), &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(2)).expect("packet");
    let pkt = String::from_utf8(bytes).unwrap();
    assert!(pkt.contains("good:7|g"));
    assert!(!pkt.contains("bad_nan"));
}

#[test]
fn special_chars_in_names_get_sanitized() {
    let (port, rx) = udp_capture();
    let snap = obj(&[("eth:0", obj(&[("rx|bytes", Value::Int(42))]))]);
    let cfg = statsd::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: None,
    };
    statsd::write(&flat(&snap), &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(2)).expect("packet");
    let pkt = String::from_utf8(bytes).unwrap();
    // ':' and '|' become '_' so they don't break the metric syntax.
    assert!(pkt.contains("glances.eth_0.rx_bytes:42|"));
}

#[test]
fn non_object_snapshot_flattens_to_no_fields() {
    // flatten::collect only understands {plugin: {...}} snapshots; a
    // bare array yields zero fields and write() sends nothing.
    let snap = Value::Array(vec![Value::Int(1)]);
    assert!(flat(&snap).is_empty());
    let (port, rx) = udp_capture();
    let cfg = statsd::Config { host: "127.0.0.1".into(), port, timestamp: None };
    statsd::write(&flat(&snap), &cfg).expect("write with no fields");
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err(),
        "no datagrams should arrive for an empty field set");
}
