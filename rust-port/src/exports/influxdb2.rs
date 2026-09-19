//! InfluxDB v2 exporter — HTTP POST to `/api/v2/write?org=<org>&bucket=<bucket>`
//! using `Authorization: Token <token>`. Body is line-protocol identical to
//! the v1 TCP exporter; this one goes over HTTP/1.1 so it can be fronted by
//! a reverse proxy.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "influxdb2";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub org: String,
    pub bucket: String,
    pub token: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8086,
            org: "glances".into(),
            bucket: "glances".into(),
            token: String::new(),
            timeout_secs: 5,
        }
    }
}

/// Build the HTTP request bytes. Exposed for unit tests.
pub fn build_request(cfg: &Config, body: &str) -> Vec<u8> {
    let mut req = Vec::with_capacity(body.len() + 256);
    let path = format!("/api/v2/write?org={}&bucket={}", url_escape(&cfg.org), url_escape(&cfg.bucket));
    let host_header = format!("{}:{}", cfg.host, cfg.port);
    write!(
        req,
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n",
        path, host_header, body.len(),
    ).unwrap();
    if !cfg.token.is_empty() {
        write!(req, "Authorization: Token {}\r\n", cfg.token).unwrap();
    }
    req.extend_from_slice(b"\r\n");
    req.extend_from_slice(body.as_bytes());
    req
}

fn url_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            out.push(ch);
        } else {
            // crude UTF-8 escape — bytes that are not unreserved chars get
            // percent-encoded; v2 accepts arbitrary UTF-8 in org/bucket
            // query params, so per-byte escaping is correct.
            let mut buf = [0u8; 4];
            for b in ch.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

fn now_nanos() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

fn field_to_lp(key: &str, v: &Value) -> Option<String> {
    let k = escape_tag(key);
    let rendered = match v {
        Value::Int(i) => format!("{}i", i),
        Value::Uint(u) => format!("{}i", u),
        Value::Float(f) if f.is_nan() || f.is_infinite() => return None,
        Value::Float(f) => format!("{}", f),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => format!("\"{}\"", escape_field_str(s)),
        Value::Null | Value::Array(_) | Value::Object(_) => return None,
    };
    Some(format!("{}={}", k, rendered))
}

fn escape_measurement(s: &str) -> String {
    s.replace(',', "\\,").replace(' ', "\\ ")
}
fn escape_tag(s: &str) -> String {
    s.replace(',', "\\,").replace('=', "\\=").replace(' ', "\\ ")
}
fn escape_field_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn build_body(snap: &Value, ts_override: Option<f64>) -> String {
    let ts = match ts_override {
        Some(s) => (s * 1e9) as i64,
        None => now_nanos(),
    };
    let plugins = match snap.as_object() { Some(o) => o, None => return String::new() };
    let mut out = String::new();
    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        let mut field_strs = Vec::new();
        for (k, v) in fields {
            if let Some(s) = field_to_lp(k, v) { field_strs.push(s); }
        }
        if field_strs.is_empty() { continue; }
        out.push_str(&escape_measurement(plugin));
        out.push(' ');
        out.push_str(&field_strs.join(","));
        out.push(' ');
        out.push_str(&ts.to_string());
        out.push('\n');
    }
    out
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.org.is_empty() || cfg.bucket.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "influxdb2 exporter requires org + bucket".into(),
        ));
    }
    let body = build_body(snap, None);
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
    fn request_targets_v2_endpoint_with_query_params() {
        let cfg = Config::default();
        let req = build_request(&cfg, "x");
        let raw = String::from_utf8(req).unwrap();
        assert!(raw.starts_with("POST /api/v2/write?org=glances&bucket=glances HTTP/1.1\r\n"));
        assert!(raw.contains("Content-Type: text/plain"));
    }

    #[test]
    fn token_emits_authorization_header() {
        let cfg = Config { token: "abc123".into(), ..Default::default() };
        let req = build_request(&cfg, "x");
        let raw = String::from_utf8(req).unwrap();
        assert!(raw.contains("Authorization: Token abc123\r\n"));
    }

    #[test]
    fn url_escape_handles_specials() {
        assert_eq!(url_escape("hello"), "hello");
        assert_eq!(url_escape("hello world"), "hello%20world");
        assert_eq!(url_escape("a/b"), "a%2Fb");
        assert_eq!(url_escape("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn body_renders_line_protocol() {
        let snap = obj(&[("cpu", obj(&[("total", Value::Int(42))]))]);
        let body = build_body(&snap, Some(1.0));
        assert!(body.starts_with("cpu total=42i "));
        assert!(body.contains(" 1000000000\n"));
    }

    #[test]
    fn empty_org_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { org: String::new(), ..Default::default() };
        let err = write(&snap, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}