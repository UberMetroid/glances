//! End-to-end smoke for the M14 HTTP server.
//!
//! Spins up the accept loop on a free port, fires real HTTP/1.1 requests
//! at it, and asserts the documented behaviors:
//!   - `/healthz` returns 200
//!   - `/api/all/values` returns 200 + JSON object
//!   - `/favicon.ico` returns 200 + binary
//!   - Unknown paths return 404
//!   - With auth_enabled, missing creds → 401; valid creds → 200
//!   - HTTP Basic parsing recovers the canonical RFC 7617 example.
//!
//! Each test gets its own listener so they don't fight for ports.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;
use std::io::Result as IoResult;

use crate::cli::args::{Args, Mode};
use crate::core::password::{PasswordFile, PasswordHash};
use crate::core::sha256::sha256_hex;
use crate::core::stats::GlancesStats;
use crate::outputs::web::auth;
use crate::outputs::web::request;
use crate::outputs::web::response::Response;
use crate::outputs::web::server;
use crate::outputs::web::sse;
use crate::outputs::web::static_fs;
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

fn get(addr: std::net::SocketAddr, path: &str) -> Vec<u8> {
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).expect("connect");
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    round_trip(stream, req.as_bytes())
}

/// Resolved-once test server address. Started lazily on first call.
fn srv_addr() -> std::net::SocketAddr { ensure_server_running().0 }

fn ensure_server_running() -> &'static (std::net::SocketAddr, std::thread::JoinHandle<IoResult<()>>) {
    use std::sync::OnceLock;
    static SERVER: OnceLock<(std::net::SocketAddr, std::thread::JoinHandle<IoResult<()>>)> = OnceLock::new();
    SERVER.get_or_init(|| {
        let stats = Arc::new(GlancesStats::new(2.0));
        plugins::register_all(&stats);
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let (addr, handle) = server::spawn_test_server_no_auth(stats, args);
        std::thread::sleep(Duration::from_millis(50));
        (addr, handle)
    })
}

#[test]
fn healthz_returns_200() {
    let resp = get(srv_addr(), "/healthz");
    let s = String::from_utf8_lossy(&resp);
    assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(s.ends_with("ok\n"));
}

#[test]
fn unknown_path_is_404() {
    let resp = get(srv_addr(), "/nope");
    let s = String::from_utf8_lossy(&resp);
    assert!(s.starts_with("HTTP/1.1 404 Not Found\r\n"));
}

#[test]
fn favicon_returns_binary() {
    let resp = get(srv_addr(), "/favicon.ico");
    assert!(resp.starts_with(b"HTTP/1.1 200 OK\r\n"));
    let s = String::from_utf8_lossy(&resp);
    assert!(s.contains("Content-Type: image/x-icon"));
    let body_start = s.find("\r\n\r\n").unwrap() + 4;
    assert!(!resp[body_start..].is_empty());
}

#[test]
fn api_all_values_is_json_object() {
    let resp = get(srv_addr(), "/api/all/values");
    let s = String::from_utf8_lossy(&resp);
    assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(s.contains("Content-Type: application/json"));
    let body_start = s.find("\r\n\r\n").unwrap() + 4;
    let body = std::str::from_utf8(&resp[body_start..]).unwrap();
    assert!(body.starts_with('{'), "body should be a JSON object: {}", &body[..body.len().min(80)]);
}

#[test]
fn request_parser_roundtrips_basic_get() {
    let raw = b"GET /api/cpu/values HTTP/1.1\r\nHost: x\r\nAccept: */*\r\n\r\n";
    let r = request::parse(raw).expect("parse");
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, "/api/cpu/values");
    assert_eq!(r.headers.get("host").unwrap(), "x");
    assert_eq!(r.headers.get("accept").unwrap(), "*/*");
}

#[test]
fn request_parser_handles_post_with_body() {
    let raw = b"POST /api HTTP/1.1\r\nContent-Length: 11\r\n\r\nhello world";
    let r = request::parse(raw).expect("parse");
    assert_eq!(r.body, b"hello world");
}

#[test]
fn response_into_bytes_has_required_headers() {
    let r = Response::ok_json("{\"x\":1}".into()).into_bytes();
    let s = String::from_utf8_lossy(&r);
    assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(s.contains("Content-Length: 7"));
    assert!(s.contains("Content-Type: application/json"));
    assert!(s.contains("Server: glances-rs"));
    assert!(s.ends_with("{\"x\":1}"));
}

#[test]
fn auth_parses_canonical_basic() {
    let (u, p) = auth::parse_basic("Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==").unwrap();
    assert_eq!(u, "Aladdin");
    assert_eq!(p, "open sesame");
}

#[test]
fn auth_rejects_bearer() { assert!(auth::parse_basic("Bearer foo").is_none()); }

#[test]
fn auth_rejects_garbage_b64() { assert!(auth::parse_basic("Basic !!!not_base64!!!").is_none()); }

#[test]
fn auth_full_round_trip_against_password_file() {
    let mut pw = PasswordFile::empty();
    pw.entries.insert("admin".into(), PasswordHash::Plain(sha256_hex(b"hunter2")));
    assert!(auth::verify(&pw, "admin", "hunter2"));
    assert!(!auth::verify(&pw, "admin", "wrong"));
    assert!(!auth::verify(&pw, "nobody", "hunter2"));
}

#[test]
fn sse_event_framing_is_well_formed() {
    let frame = sse::format_event("stats", r#"{"cpu":1.0}"#, Some(7));
    assert!(frame.starts_with("event: stats\n"));
    assert!(frame.contains(r#"data: {"cpu":1.0}"#));
    assert!(frame.contains("id: 7\n"));
    assert!(frame.ends_with("\n\n"));
}

#[test]
fn static_fs_lookups() {
    assert!(static_fs::lookup_path("/").is_some());
    assert!(static_fs::lookup_path("/favicon.ico").is_some());
    assert!(static_fs::lookup_path("/missing.png").is_none());
    assert!(static_fs::lookup_path("/static/dashboard.html").is_some());
}

/// Auth-enabled variant: separate listener because we need a different
/// ServerState. We can't reuse the shared server for this test.
#[test]
fn auth_required_when_enabled_no_creds() {
    let stats = Arc::new(GlancesStats::new(2.0));
    let args = Args { mode: Mode::WebServer, auth_enabled: true, ..Args::default() };
    let (addr, _h) = server::spawn_test_server_no_auth(stats, args);
    std::thread::sleep(Duration::from_millis(50));
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).expect("connect");
    let raw = b"GET /api/all/values HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let resp = round_trip(stream, raw);
    let s = String::from_utf8_lossy(&resp);
    assert!(s.starts_with("HTTP/1.1 401 Unauthorized\r\n"),
            "expected 401, got: {}", &s[..s.len().min(80)]);
    assert!(s.contains("WWW-Authenticate: Basic"));
}

#[test]
fn auth_required_when_enabled_correct_creds() {
    let mut pw = PasswordFile::empty();
    pw.entries.insert("admin".into(), PasswordHash::Plain(sha256_hex(b"hunter2")));
    let stats = Arc::new(GlancesStats::new(2.0));
    plugins::register_all(&stats);
    let args = Args { mode: Mode::WebServer, auth_enabled: true, ..Args::default() };
    let (addr, _h) = server::spawn_test_server(stats, args, pw);
    std::thread::sleep(Duration::from_millis(50));
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).expect("connect");
    let basic = format!("Basic {}", base64_encode(b"admin:hunter2"));
    let raw = format!(
        "GET /healthz HTTP/1.1\r\nHost: localhost\r\nAuthorization: {basic}\r\nConnection: close\r\n\r\n"
    );
    let resp = round_trip(stream, raw.as_bytes());
    let s = String::from_utf8_lossy(&resp);
    assert!(s.starts_with("HTTP/1.1 200 OK\r\n"),
            "expected 200 with valid creds, got: {}", &s[..s.len().min(80)]);
}

/// Tiny base64 encoder (RFC 4648 §4). Std-only.
fn base64_encode(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let chunks = input.chunks(3);
    let mut last_len = 0;
    for chunk in chunks {
        last_len = chunk.len();
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(ALPHA[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 { out.push(ALPHA[((n >> 6) & 0x3f) as usize] as char); }
        if chunk.len() > 2 { out.push(ALPHA[(n & 0x3f) as usize] as char); }
    }
    if last_len == 1 { out.push('='); out.push('='); }
    else if last_len == 2 { out.push('='); }
    out
}
