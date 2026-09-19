//! ClickHouse HTTP exporter — `POST /` with `query=INSERT ... FORMAT
//! JSONEachRow`. Each refresh tick renders one JSON object per (plugin,
//! key) tuple, newline-delimited, and POSTs them to the configured HTTP
//! endpoint. PARTIAL: ships the request body but does not consume the
//! response body — ClickHouse is fire-and-forget on success.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::{to_json, Value};

pub const NAME: &str = "clickhouse";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub table: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8123,
            database: "default".into(),
            table: "glances_stats".into(),
            timeout_secs: 5,
        }
    }
}

/// Build the HTTP request bytes (headers + body) for the JSONEachRow
/// INSERT. Exposed for unit tests.
pub fn build_request(cfg: &Config, body: &str) -> Vec<u8> {
    let mut req = Vec::with_capacity(body.len() + 256);
    let path = format!(
        "/?query=INSERT+INTO+{}.{}+FORMAT+JSONEachRow",
        cfg.database, cfg.table,
    );
    let host_header = format!("{}:{}", cfg.host, cfg.port);
    write!(
        req,
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        path, host_header, body.len(),
    ).unwrap();
    req.extend_from_slice(body.as_bytes());
    req
}

fn render_row(plugin: &str, key: &str, value: &Value) -> String {
    // Flatten a (plugin, key, value) tuple into a single JSON object so
    // JSONEachRow can ingest it. The numeric form is taken from to_json.
    let s = to_json(value);
    format!(
        "{{\"plugin\":{},\"key\":{},\"value\":{}}}",
        quote(plugin),
        quote(key),
        s,
    )
}

fn quote(s: &str) -> String {
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

pub(crate) fn build_body(snap: &Value) -> String {
    let plugins = match snap.as_object() { Some(o) => o, None => return String::new() };
    let mut out = String::new();
    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        for (k, v) in fields {
            // Skip NaN / Infinity floats — ClickHouse rejects them in
            // numeric columns.
            if let Value::Float(f) = v {
                if f.is_nan() || f.is_infinite() { continue; }
            }
            out.push_str(&render_row(plugin, k, v));
            out.push('\n');
        }
    }
    out
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.table.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "clickhouse exporter requires table".into(),
        ));
    }
    let body = build_body(snap);
    if body.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let req = build_request(cfg, &body);
    let timeout = Duration::from_secs(cfg.timeout_secs);
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    let mut s = stream;
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
    fn build_request_contains_insert_query() {
        let cfg = Config::default();
        let req = build_request(&cfg, "{\"plugin\":\"cpu\"}\n");
        let raw = String::from_utf8(req).unwrap();
        assert!(raw.starts_with("POST /?query=INSERT+INTO+default.glances_stats+FORMAT+JSONEachRow HTTP/1.1\r\n"));
        assert!(raw.contains("Content-Type: application/x-ndjson"));
        assert!(raw.ends_with("{\"plugin\":\"cpu\"}\n"));
    }

    #[test]
    fn content_length_matches_body() {
        let cfg = Config::default();
        let body = "{\"x\":1}\n";
        let req = build_request(&cfg, body);
        let raw = String::from_utf8(req).unwrap();
        let head = raw.split("\r\n\r\n").next().unwrap();
        let declared = head
            .lines()
            .find(|l| l.starts_with("Content-Length:"))
            .and_then(|l| l.split(':').nth(1))
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap();
        assert_eq!(declared, body.len());
    }

    #[test]
    fn nan_floats_are_dropped_from_body() {
        let snap = obj(&[(
            "cpu",
            obj(&[("bad", Value::Float(f64::NAN)), ("ok", Value::Int(1))]),
        )]);
        let body = build_body(&snap);
        assert!(!body.contains("bad"));
        assert!(body.contains("\"ok\""));
    }

    #[test]
    fn quote_escapes_special_chars() {
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote("a\\b"), "\"a\\\\b\"");
        assert_eq!(quote("a\nb"), "\"a\\nb\"");
    }

    #[test]
    fn empty_table_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { table: String::new(), ..Default::default() };
        let err = write(&snap, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}