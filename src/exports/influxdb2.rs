//! InfluxDB v2 exporter — HTTP POST to `/api/v2/write?org=<org>&bucket=<bucket>`
//! using `Authorization: Token <token>`. Body is line-protocol identical
//! to the v1 exporter (plus `u`-suffixed unsigned fields, which v1 lacks).
//! When `file` is set, the body is appended to that path instead.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "influxdb2";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub org: String,
    pub bucket: String,
    pub token: String,
    /// When set, append the LP body to this file instead of POSTing.
    pub file: Option<String>,
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
            file: None,
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
        // v2 LP supports the `u` unsigned suffix — no i64 wraparound.
        Value::Uint(u) => format!("{}u", u),
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

/// Group flat fields into `measurement field=v,… ts` lines.
pub(crate) fn build_body(fields: &[Field<'_>], ts_override: Option<f64>) -> String {
    let ts = match ts_override {
        Some(s) => (s * 1e9) as i64,
        None => now_nanos(),
    };
    let mut out = String::new();
    let mut cur_series = String::new();
    let mut cur_fields: Vec<String> = Vec::new();
    let flush = |series: &str, fs: &mut Vec<String>, out: &mut String| {
        if fs.is_empty() { return; }
        out.push_str(&escape_measurement(series));
        out.push(' ');
        out.push_str(&fs.join(","));
        out.push(' ');
        out.push_str(&ts.to_string());
        out.push('\n');
        fs.clear();
    };
    for f in fields {
        if f.series != cur_series {
            flush(&cur_series, &mut cur_fields, &mut out);
            cur_series = f.series.clone();
        }
        if let Some(s) = field_to_lp(f.key, f.value) { cur_fields.push(s); }
    }
    flush(&cur_series, &mut cur_fields, &mut out);
    out
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let body = build_body(fields, None);
    if body.is_empty() { return Ok(()); }
    if let Some(path) = cfg.file.as_ref() {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        f.write_all(body.as_bytes())?;
        return Ok(());
    }
    if cfg.org.is_empty() || cfg.bucket.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "influxdb2 exporter requires org + bucket".into(),
        ));
    }

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
    use std::collections::{BTreeMap, HashMap};

    fn obj(pairs: &[(&str, Value)]) -> Value {
        let mut m = BTreeMap::new();
        for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
        Value::Object(m)
    }

    fn flat(snap: &Value) -> Vec<Field<'_>> {
        crate::exports::flatten::collect(snap, &HashMap::new())
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
        let body = build_body(&flat(&snap), Some(1.0));
        assert!(body.starts_with("cpu total=42i "));
        assert!(body.contains(" 1000000000\n"));
    }

    #[test]
    fn unsigned_uses_u_suffix() {
        let snap = obj(&[("x", obj(&[("big", Value::Uint(u64::MAX))]))]);
        let body = build_body(&flat(&snap), Some(1.0));
        assert!(body.contains("big=18446744073709551615u"), "got: {}", body);
    }

    #[test]
    fn empty_org_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { org: String::new(), ..Default::default() };
        let err = write(&flat(&snap), &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}
