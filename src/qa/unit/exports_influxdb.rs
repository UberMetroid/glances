//! Unit tests for the InfluxDB v1 line-protocol exporter.
//!
//! Uses `std::net::TcpListener::bind("127.0.0.1:0")` to grab a free
//! ephemeral port and `TcpStream::set_read_timeout` so the test never
//! hangs if the server side fails.

use std::collections::BTreeMap;
use std::io::Read;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::core::value::Value;
use crate::exports::influxdb;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

/// Spin up a one-shot TCP server, return (port, Receiver<Vec<u8>>).
/// The server thread accepts exactly one connection, reads until the
/// client closes, and returns the captured bytes via the channel.
fn one_shot_server() -> (u16, mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let _ = stream.read_to_end(&mut buf);
            let _ = tx.send(buf);
        }
    });
    (port, rx)
}

#[test]
fn integer_field_renders_with_i_suffix() {
    let (port, rx) = one_shot_server();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(42))]))]);
    let cfg = influxdb::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: Some(1.0),
    };
    influxdb::write(&snap, &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let line = String::from_utf8(bytes).unwrap();
    assert!(line.starts_with("cpu total=42i "));
    // 1.0 s = 1_000_000_000 ns
    assert!(line.contains(" 1000000000\n"));
}

#[test]
fn float_field_renders_with_decimals() {
    let (port, rx) = one_shot_server();
    let snap = obj(&[("cpu", obj(&[("user", Value::Float(3.14))]))]);
    let cfg = influxdb::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: Some(0.0),
    };
    influxdb::write(&snap, &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let line = String::from_utf8(bytes).unwrap();
    assert!(line.starts_with("cpu user=3.14 "));
}

#[test]
fn string_field_renders_with_quotes_and_escapes() {
    let (port, rx) = one_shot_server();
    let snap = obj(&[("cpu", obj(&[("msg", Value::String("a\\b\"c".into()))]))]);
    let cfg = influxdb::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: Some(0.0),
    };
    influxdb::write(&snap, &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let line = String::from_utf8(bytes).unwrap();
    assert!(line.contains("msg=\"a\\\\b\\\"c\""));
}

#[test]
fn nan_and_inf_values_are_dropped() {
    let (port, rx) = one_shot_server();
    let snap = obj(&[(
        "cpu",
        obj(&[
            ("bad_nan", Value::Float(f64::NAN)),
            ("bad_inf", Value::Float(f64::INFINITY)),
        ]),
    )]);
    let cfg = influxdb::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: Some(0.0),
    };
    influxdb::write(&snap, &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let line = String::from_utf8(bytes).unwrap();
    // Both fields are skipped, so the line has only the timestamp column
    // after a single space — i.e. "cpu  0\n".
    assert_eq!(line, "cpu  0\n");
}

#[test]
fn connection_failure_propagates_after_backoff() {
    // Bind a port, then drop the listener so the connection will be
    // refused. The exporter sleeps 5s on retry, so we shrink the wait
    // by binding to a high port that nothing is listening on.
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let cfg = influxdb::Config {
        host: "127.0.0.1".into(),
        port: 1, // privileged port nothing is bound to
        timestamp: Some(0.0),
    };
    let start = std::time::Instant::now();
    let res = influxdb::write(&snap, &cfg);
    let elapsed = start.elapsed();
    assert!(res.is_err());
    // 5s backoff should have elapsed before the second attempt fails.
    assert!(elapsed >= Duration::from_secs(5), "elapsed was {:?}", elapsed);
}