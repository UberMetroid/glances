//! CouchDB exporter — POST one JSON document per (plugin, key) tuple to
//! a CouchDB database (URL `<scheme>://<host>:<port>/<database>/`).
//!
//! Each document carries `plugin`, `key`, `value`, and a server-assigned
//! `_id` (CouchDB rejects duplicate `_id`s without `_rev`, so we leave
//! it off and let CouchDB assign one).

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::{to_json, Value};
use crate::exports::flatten::Field;

pub const NAME: &str = "couchdb";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub auth_user: Option<String>,
    pub auth_pass: Option<String>,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 5984,
            database: "glances".into(),
            auth_user: None,
            auth_pass: None,
            timeout_secs: 5,
        }
    }
}

/// Build the HTTP request bytes. Exposed for unit tests.
pub fn build_request(cfg: &Config, body: &str) -> Vec<u8> {
    let mut req = Vec::with_capacity(body.len() + 256);
    // One `_bulk_docs` call per tick — the old code POSTed several
    // concatenated JSON docs to `/<db>/`, which CouchDB rejects as a
    // single malformed document body.
    let path = format!("/{}/_bulk_docs", cfg.database);
    let host_header = format!("{}:{}", cfg.host, cfg.port);
    write!(
        req,
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        path, host_header, body.len(),
    ).unwrap();
    if let (Some(u), Some(p)) = (cfg.auth_user.as_ref(), cfg.auth_pass.as_ref()) {
        let raw = format!("{}:{}", u, p);
        let encoded = base64_encode(raw.as_bytes());
        write!(req, "Authorization: Basic {}\r\n", encoded).unwrap();
    }
    req.extend_from_slice(b"\r\n");
    req.extend_from_slice(body.as_bytes());
    req
}

/// Tiny RFC 4648 base64 encoder (no padding confusion; std has none).
fn base64_encode(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

/// `{"docs":[{…},…]}` — one `_bulk_docs` envelope carrying every field.
pub(crate) fn build_body(fields: &[Field<'_>]) -> String {
    let mut out = String::from("{\"docs\":[");
    let mut first = true;
    for f in fields {
        if let Value::Float(v) = f.value {
            if v.is_nan() || v.is_infinite() { continue; }
        }
        if !first { out.push(','); }
        first = false;
        out.push_str(&format!(
            "{{\"plugin\":{},\"series\":{},\"key\":{},\"value\":{}}}",
            quote_str(f.plugin),
            quote_str(&f.series),
            quote_str(f.key),
            to_json(f.value),
        ));
    }
    out.push_str("]}");
    if first { String::new() } else { out }
}

fn quote_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    if cfg.database.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "couchdb exporter requires database".into(),
        ));
    }
    let body = build_body(fields);
    if body.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    let mut s = stream;
    let req = build_request(cfg, &body);
    s.write_all(&req)?;
    s.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn obj(pairs: &[(&str, Value)]) -> Value {
        let mut m = BTreeMap::new();
        for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
        Value::Object(m)
    }

    #[test]
    fn build_request_targets_bulk_docs_endpoint() {
        let cfg = Config::default();
        let req = build_request(&cfg, "{}");
        let raw = String::from_utf8(req).unwrap();
        assert!(raw.starts_with("POST /glances/_bulk_docs HTTP/1.1\r\n"));
        assert!(raw.contains("Host: 127.0.0.1:5984"));
        assert!(raw.contains("Content-Type: application/json"));
        assert!(!raw.contains("Authorization"));
    }

    #[test]
    fn auth_emits_basic_header() {
        let cfg = Config {
            auth_user: Some("alice".into()),
            auth_pass: Some("s3cret".into()),
            ..Default::default()
        };
        let req = build_request(&cfg, "{}");
        let raw = String::from_utf8(req).unwrap();
        // alice:s3cret → base64("alice:s3cret") = "YWxpY2U6czNjcmV0"
        assert!(raw.contains("Authorization: Basic YWxpY2U6czNjcmV0"));
    }

    #[test]
    fn base64_encoder_handles_padding() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn nan_floats_skipped_from_body() {
        let snap = obj(&[(
            "cpu",
            obj(&[("bad", Value::Float(f64::NAN)), ("ok", Value::Int(1))]),
        )]);
        let fields = crate::exports::flatten::collect(&snap, &Default::default());
        let body = build_body(&fields);
        assert!(!body.contains("bad"));
        assert!(body.contains("\"ok\""));
    }

    #[test]
    fn body_is_bulk_docs_envelope() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let fields = crate::exports::flatten::collect(&snap, &Default::default());
        let body = build_body(&fields);
        assert!(body.starts_with("{\"docs\":[{"));
        assert!(body.ends_with("]}"));
        assert!(!body.contains('\n'), "envelope must be a single JSON object");
    }

    #[test]
    fn empty_database_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { database: String::new(), ..Default::default() };
        let fields = crate::exports::flatten::collect(&snap, &Default::default());
        let err = write(&fields, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}