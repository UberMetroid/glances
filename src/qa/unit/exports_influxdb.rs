//! Unit tests for the InfluxDB v1 line-protocol exporter.
//!
//! v1 is an HTTP exporter: `POST /write?db=<db>` to `host:port`, or an
//! append to `cfg.file` when the file sink is configured. Tests use a
//! one-shot TCP listener to capture the request, and TempDir files for
//! the file sink.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::influxdb;
use crate::qa::harness::TempDir;

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

fn file_cfg(path: &str, ts: f64) -> influxdb::Config {
    influxdb::Config {
        file: Some(path.to_string()),
        timestamp: Some(ts),
        ..Default::default()
    }
}

#[test]
fn posts_line_protocol_to_write_endpoint() {
    let (port, rx) = one_shot_server();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(42))]))]);
    let cfg = influxdb::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: Some(1.0),
        ..Default::default()
    };
    influxdb::write(&flat(&snap), &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let req = String::from_utf8(bytes).unwrap();
    assert!(req.starts_with("POST /write?db=glances HTTP/1.1\r\n"), "got: {}", req);
    assert!(req.contains("Content-Type: text/plain"));
    // 1.0 s = 1_000_000_000 ns.
    assert!(req.contains("cpu total=42i 1000000000\n"), "got: {}", req);
}

#[test]
fn integer_field_renders_with_i_suffix() {
    let dir = TempDir::new("influxdb-int");
    let path = dir.path().join("out.lp").to_string_lossy().to_string();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(42))]))]);
    influxdb::write(&flat(&snap), &file_cfg(&path, 1.0)).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert_eq!(body, "cpu total=42i 1000000000\n");
}

#[test]
fn float_field_renders_with_decimals() {
    let dir = TempDir::new("influxdb-float");
    let path = dir.path().join("out.lp").to_string_lossy().to_string();
    let snap = obj(&[("cpu", obj(&[("user", Value::Float(3.14))]))]);
    influxdb::write(&flat(&snap), &file_cfg(&path, 0.0)).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert_eq!(body, "cpu user=3.14 0\n");
}

#[test]
fn string_field_renders_with_quotes_and_escapes() {
    let dir = TempDir::new("influxdb-str");
    let path = dir.path().join("out.lp").to_string_lossy().to_string();
    let snap = obj(&[("cpu", obj(&[("msg", Value::String("a\\b\"c".into()))]))]);
    influxdb::write(&flat(&snap), &file_cfg(&path, 0.0)).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert!(body.contains("msg=\"a\\\\b\\\"c\""), "got: {}", body);
}

#[test]
fn nan_and_inf_values_emit_no_line_at_all() {
    // A series whose fields are all non-finite produces no LP line —
    // `series  <ts>` is invalid line protocol. With the file sink the
    // body is empty, so the file is never even created.
    let dir = TempDir::new("influxdb-nan");
    let path = dir.path().join("out.lp").to_string_lossy().to_string();
    let snap = obj(&[(
        "cpu",
        obj(&[
            ("bad_nan", Value::Float(f64::NAN)),
            ("bad_inf", Value::Float(f64::INFINITY)),
        ]),
    )]);
    influxdb::write(&flat(&snap), &file_cfg(&path, 0.0)).expect("write");
    let body = fs::read_to_string(&path).unwrap_or_default();
    assert!(body.is_empty(), "expected no LP output, got: {:?}", body);
}

#[test]
fn empty_database_without_file_rejected() {
    // Without a file sink, `?db=` needs a database name.
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let cfg = influxdb::Config {
        database: String::new(),
        timestamp: Some(0.0),
        ..Default::default()
    };
    let err = influxdb::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn connection_failure_propagates() {
    // Bind then drop a listener so the connection is refused.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let cfg = influxdb::Config {
        host: "127.0.0.1".into(),
        port,
        timestamp: Some(0.0),
        timeout_secs: 1,
        ..Default::default()
    };
    let res = influxdb::write(&flat(&snap), &cfg);
    assert!(res.is_err());
}
