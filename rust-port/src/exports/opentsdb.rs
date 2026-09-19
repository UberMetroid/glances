//! OpenTSDB `telnet put` exporter — connects to a TSDB node and emits
//! one `put <metric> <timestamp> <value> [<tagk>=<tagv> ...]\n` line per
//! (plugin, key) tuple. NaN / Infinity are skipped; non-numeric values
//! are coerced to their numeric form where possible.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "opentsdb";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 4242,
            timeout_secs: 5,
        }
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Build one `put` line for a (plugin, key, value) triple.
pub fn render_put(plugin: &str, key: &str, value: f64, ts: i64) -> Option<String> {
    if value.is_nan() || value.is_infinite() { return None; }
    Some(format!(
        "put {}.{} {} {} plugin={}\n",
        sanitize(plugin),
        sanitize(key),
        ts,
        value,
        sanitize(plugin),
    ))
}

fn sanitize(s: &str) -> String {
    // OpenTSDB tag keys/values and metric names cannot contain spaces.
    s.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect()
}

pub fn build_body(snap: &Value, ts: i64) -> String {
    let plugins = match snap.as_object() { Some(o) => o, None => return String::new() };
    let mut out = String::new();
    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        for (k, v) in fields {
            let n = match v {
                Value::Int(i) => Some(*i as f64),
                Value::Uint(u) => Some(*u as f64),
                Value::Float(f) if f.is_nan() || f.is_infinite() => None,
                Value::Float(f) => Some(*f),
                _ => None,
            };
            if let Some(n) = n {
                if let Some(line) = render_put(plugin, k, n, ts) {
                    out.push_str(&line);
                }
            }
        }
    }
    out
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    let body = build_body(snap, now_secs());
    if body.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    let mut s = stream;
    s.write_all(body.as_bytes())?;
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
    fn render_put_produces_put_command() {
        let line = render_put("cpu", "total", 42.0, 1_700_000_000).unwrap();
        assert_eq!(line, "put cpu.total 1700000000 42 plugin=cpu\n");
    }

    #[test]
    fn render_put_sanitizes_whitespace() {
        let line = render_put("cpu 0", "rx bytes", 1.0, 100).unwrap();
        // Whitespace in plugin/key is collapsed to underscores.
        assert!(line.starts_with("put cpu_0.rx_bytes 100 1 plugin=cpu_0\n"));
    }

    #[test]
    fn render_put_skips_nan_and_inf() {
        assert!(render_put("cpu", "x", f64::NAN, 0).is_none());
        assert!(render_put("cpu", "x", f64::INFINITY, 0).is_none());
        assert!(render_put("cpu", "x", f64::NEG_INFINITY, 0).is_none());
    }

    #[test]
    fn build_body_emits_one_put_per_numeric_field() {
        let snap = obj(&[(
            "cpu",
            obj(&[
                ("good", Value::Int(1)),
                ("bad", Value::Float(f64::NAN)),
                ("str", Value::String("hi".into())),
            ]),
        )]);
        let body = build_body(&snap, 100);
        assert!(body.contains("put cpu.good 100 1 plugin=cpu\n"));
        assert!(!body.contains("bad"));
        // String values are skipped (not coercible to numeric).
        assert!(!body.contains("str"));
    }

    #[test]
    fn empty_snapshot_yields_empty_body() {
        let snap = Value::Object(BTreeMap::new());
        let body = build_body(&snap, 100);
        assert!(body.is_empty());
    }
}