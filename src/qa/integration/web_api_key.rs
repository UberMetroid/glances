//! API-key gate end-to-end: `X-API-Key` on data routes, dashboard
//! shell open in key-only mode, no Basic challenge, Basic coexists.
//!
//! Each test gets its own listener so they don't fight for ports.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use crate::cli::args::{Args, Mode};
use crate::core::password::{PasswordFile, PasswordHash};
use crate::core::sha256::sha256_hex;
use crate::core::stats::GlancesStats;
use crate::outputs::web::server;
use crate::plugins;

/// Send `raw`, read until the server closes, return the response bytes.
fn round_trip(stream: TcpStream, raw: &[u8]) -> Vec<u8> {
    let mut s = stream;
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
    s.write_all(raw).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    out
}

fn get(addr: std::net::SocketAddr, path: &str, key: Option<&str>) -> String {
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).expect("connect");
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n");
    if let Some(k) = key {
        req.push_str(&format!("X-API-Key: {k}\r\n"));
    }
    req.push_str("Connection: close\r\n\r\n");
    String::from_utf8_lossy(&round_trip(stream, req.as_bytes())).into_owned()
}

fn head(s: &str) -> &str {
    &s[..s.len().min(80)]
}

fn keyed_server(key: Option<String>) -> std::net::SocketAddr {
    let stats = Arc::new(GlancesStats::new(2.0));
    plugins::register_all(&stats);
    let args = Args { mode: Mode::WebServer, ..Args::default() };
    let (addr, _h) = server::spawn_test_server(stats, args, PasswordFile::empty(), key);
    std::thread::sleep(Duration::from_millis(50));
    addr
}

#[test]
fn key_gate_api_401_shell_open() {
    let addr = keyed_server(Some("k3y".into()));
    let no_key = get(addr, "/api/4/cpu", None);
    assert!(no_key.starts_with("HTTP/1.1 401 Unauthorized\r\n"), "{}", head(&no_key));
    assert!(!no_key.contains("WWW-Authenticate"), "key-only 401 must not challenge Basic");
    let wrong = get(addr, "/api/4/cpu", Some("nope"));
    assert!(wrong.starts_with("HTTP/1.1 401"), "{}", head(&wrong));
    let shell = get(addr, "/", None);
    assert!(shell.starts_with("HTTP/1.1 200 OK\r\n"), "shell stays open, got: {}", head(&shell));
    let ok = get(addr, "/api/4/cpu", Some("k3y"));
    assert!(ok.starts_with("HTTP/1.1 200 OK\r\n"), "{}", head(&ok));
}

#[test]
fn key_and_basic_either_passes() {
    let mut pw = PasswordFile::empty();
    pw.entries.insert("admin".into(), PasswordHash::Plain(sha256_hex(b"hunter2")));
    let stats = Arc::new(GlancesStats::new(2.0));
    plugins::register_all(&stats);
    let args = Args { mode: Mode::WebServer, auth_enabled: true, ..Args::default() };
    let (addr, _h) = server::spawn_test_server(stats, args, pw, Some("k3y".into()));
    std::thread::sleep(Duration::from_millis(50));
    let via_key = get(addr, "/api/4/cpu", Some("k3y"));
    assert!(via_key.starts_with("HTTP/1.1 200 OK\r\n"), "{}", head(&via_key));
    // "admin:hunter2" — verified encoding, mirrors web_api_smoke.
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).expect("connect");
    let raw = b"GET /api/4/cpu HTTP/1.1\r\nHost: localhost\r\nAuthorization: Basic YWRtaW46aHVudGVyMg==\r\nConnection: close\r\n\r\n";
    let via_basic = String::from_utf8_lossy(&round_trip(stream, raw)).into_owned();
    assert!(via_basic.starts_with("HTTP/1.1 200 OK\r\n"), "{}", head(&via_basic));
}
