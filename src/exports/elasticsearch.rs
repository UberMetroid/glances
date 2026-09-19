//! Elasticsearch `_bulk` exporter — POST newline-delimited JSON actions
//! + documents to `POST /_bulk`. Each refresh tick emits one action
//! (`{"index":{"_index":"<idx>","_id":"<plugin>.<key>"}}`) followed by
//! the document, both terminated by `\n`. ES requires a trailing `\n`
//! after the last document.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "elasticsearch";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub index: String,
    pub auth_user: Option<String>,
    pub auth_pass: Option<String>,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9200,
            index: "glances".into(),
            auth_user: None,
            auth_pass: None,
            timeout_secs: 5,
        }
    }
}

/// Build the HTTP request bytes. Exposed for unit tests.
pub fn build_request(cfg: &Config, body: &str) -> Vec<u8> {
    let mut req = Vec::with_capacity(body.len() + 256);
    let host_header = format!("{}:{}", cfg.host, cfg.port);
    write!(
        req,
        "POST /_bulk HTTP/1.1\r\nHost: {}\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n",
        host_header, body.len(),
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

/// Quote a JSON string key. Mirrors the helper in `couchdb.rs`.
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

fn json_primitive(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) if f.is_nan() || f.is_infinite() => "null".into(),
        Value::Float(f) => format!("{}", f),
        Value::String(s) => quote_str(s),
        Value::Array(_) | Value::Object(_) => quote_str(&crate::core::value::to_json(v)),
    }
}

pub(crate) fn build_body(snap: &Value, index: &str) -> String {
    let plugins = match snap.as_object() { Some(o) => o, None => return String::new() };
    let mut out = String::new();
    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        for (k, v) in fields {
            let id = format!("{}.{}", plugin, k);
            let action = format!(
                "{{\"index\":{{\"_index\":{},\"_id\":{}}}}}\n",
                quote_str(index), quote_str(&id),
            );
            let doc = format!(
                "{{\"plugin\":{},\"key\":{},\"value\":{}}}\n",
                quote_str(plugin), quote_str(k), json_primitive(v),
            );
            out.push_str(&action);
            out.push_str(&doc);
        }
    }
    // ES requires the final newline even after the last document.
    out.push('\n');
    out
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.index.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "elasticsearch exporter requires index".into(),
        ));
    }
    let body = build_body(snap, &cfg.index);
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
    fn request_targets_bulk_endpoint() {
        let cfg = Config::default();
        let req = build_request(&cfg, "");
        let raw = String::from_utf8(req).unwrap();
        assert!(raw.starts_with("POST /_bulk HTTP/1.1\r\n"));
        assert!(raw.contains("Content-Type: application/x-ndjson"));
    }

    #[test]
    fn body_is_ndjson_pairs_with_trailing_newline() {
        let cfg = Config::default();
        let snap = obj(&[("cpu", obj(&[("total", Value::Int(42))]))]);
        let body = build_body(&snap, &cfg.index);
        // Each (action + doc) = 2 lines, plus the mandatory trailing newline.
        assert!(body.ends_with("\n\n"));
        let lines: Vec<&str> = body.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"_index\":\"glances\""));
        assert!(lines[0].contains("\"_id\":\"cpu.total\""));
        assert!(lines[1].contains("\"plugin\":\"cpu\""));
        assert!(lines[1].contains("\"value\":42"));
    }

    #[test]
    fn nan_values_render_as_null_in_doc() {
        let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
        let body = build_body(&snap, "glances");
        assert!(body.contains("\"value\":null"));
    }

    #[test]
    fn auth_header_emitted_when_credentials_set() {
        let cfg = Config {
            auth_user: Some("u".into()),
            auth_pass: Some("p".into()),
            ..Default::default()
        };
        let req = build_request(&cfg, "");
        let raw = String::from_utf8(req).unwrap();
        // base64("u:p") = "dTpw"
        assert!(raw.contains("Authorization: Basic dTpw"));
    }

    #[test]
    fn empty_index_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { index: String::new(), ..Default::default() };
        let err = write(&snap, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}