//! Unit tests for the RESTful HTTP POST exporter.

use std::collections::BTreeMap;
use std::io::Read;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::core::value::Value;
use crate::exports::restful;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

/// Capture one HTTP request: bind a TCP socket, return (port, receiver
/// of the raw bytes sent by the client).
fn capture_one() -> (u16, mpsc::Receiver<Vec<u8>>) {
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
fn posts_json_with_correct_headers() {
    let (port, rx) = capture_one();
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(12.5))]))]);
    let cfg = restful::Config {
        host: "127.0.0.1".into(),
        port,
        path: "/api/v1/snapshot".into(),
        auth_token: None,
        timeout_secs: 2,
    };
    restful::write(&snap, &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let raw = String::from_utf8(bytes).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").expect("header terminator");
    assert!(head.starts_with("POST /api/v1/snapshot HTTP/1.1\r\n"));
    assert!(head.contains("Host: 127.0.0.1:"));
    assert!(head.contains("Content-Type: application/json"));
    assert!(head.contains("Content-Length: "));
    assert!(head.contains("Connection: close"));
    // Body is a JSON object (no NaN handling needed for Float(12.5)).
    assert!(body.starts_with("{"));
    assert!(body.contains("\"cpu\""));
}

#[test]
fn auth_token_emits_bearer_header() {
    let (port, rx) = capture_one();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(7))]))]);
    let cfg = restful::Config {
        host: "127.0.0.1".into(),
        port,
        path: "/api/v1/snapshot".into(),
        auth_token: Some("secret-token".into()),
        timeout_secs: 2,
    };
    restful::write(&snap, &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let raw = String::from_utf8(bytes).unwrap();
    assert!(raw.contains("Authorization: Bearer secret-token"));
}

#[test]
fn empty_path_defaults_to_slash() {
    let (port, rx) = capture_one();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(1))]))]);
    let cfg = restful::Config {
        host: "127.0.0.1".into(),
        port,
        path: String::new(),
        auth_token: None,
        timeout_secs: 2,
    };
    restful::write(&snap, &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let raw = String::from_utf8(bytes).unwrap();
    assert!(raw.starts_with("POST / HTTP/1.1\r\n"));
}

#[test]
fn nan_in_snapshot_serializes_as_null() {
    let (port, rx) = capture_one();
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
    let cfg = restful::Config {
        host: "127.0.0.1".into(),
        port,
        path: "/api".into(),
        auth_token: None,
        timeout_secs: 2,
    };
    restful::write(&snap, &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let raw = String::from_utf8(bytes).unwrap();
    let (_, body) = raw.split_once("\r\n\r\n").unwrap();
    assert!(body.contains("\"bad\":null"));
}

#[test]
fn connection_failure_returns_error_promptly() {
    // Bind a port, drop the listener immediately so connect fails fast.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let snap = obj(&[("cpu", obj(&[("total", Value::Int(1))]))]);
    let cfg = restful::Config {
        host: "127.0.0.1".into(),
        port,
        path: "/api".into(),
        auth_token: None,
        timeout_secs: 2,
    };
    let start = std::time::Instant::now();
    let res = restful::write(&snap, &cfg);
    let elapsed = start.elapsed();
    assert!(res.is_err());
    // No in-write retry/backoff: the error bubbles up to the refresh
    // loop (a sleep here would stall every *other* export target).
    assert!(elapsed < Duration::from_secs(5), "elapsed: {:?}", elapsed);
}

#[test]
fn content_length_matches_body() {
    let (port, rx) = capture_one();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(1))]))]);
    let cfg = restful::Config {
        host: "127.0.0.1".into(),
        port,
        path: "/".into(),
        auth_token: None,
        timeout_secs: 2,
    };
    restful::write(&snap, &cfg).expect("write");
    let bytes = rx.recv_timeout(Duration::from_secs(3)).expect("bytes");
    let raw = String::from_utf8(bytes).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let declared = head
        .lines()
        .find(|l| l.starts_with("Content-Length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|s| s.trim().parse::<usize>().ok())
        .expect("Content-Length");
    assert_eq!(declared, body.len());
}