//! Graphite plaintext exporter — connects to a Carbon endpoint and emits
//! one `<prefix>.<series>.<key> <value> <timestamp>\n` line per numeric
//! field. Mirrors `glances/exports/glances_graphite/__init__.py`
//! (graphitesend): metric names are lowercased, spaces become `_`, and
//! only numbers are sent.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "graphite";

/// Default Carbon plaintext port.
pub const DEFAULT_PORT: u16 = 2003;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub prefix: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: DEFAULT_PORT,
            prefix: "glances".into(),
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

/// Graphite metric charset is `[a-z0-9_.-]` here (graphitesend lowercases
/// names); spaces and anything else become `_`.
pub fn sanitize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Build one plaintext line for a numeric field, or `None` for
/// non-numeric / NaN / infinite values.
pub fn render_line(prefix: &str, f: &Field<'_>, value: f64, ts: i64) -> Option<String> {
    if value.is_nan() || value.is_infinite() {
        return None;
    }
    // Display whole floats without a trailing `.0` (Carbon accepts both;
    // integers keep the line stable for tests and dashboards).
    let val = if value.fract() == 0.0 && value.abs() < 9e15 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    };
    Some(format!(
        "{}.{}.{} {} {}\n",
        sanitize(prefix),
        sanitize(&f.series),
        sanitize(f.key),
        val,
        ts
    ))
}

pub fn build_body(fields: &[Field<'_>], cfg: &Config, ts: i64) -> String {
    let mut out = String::new();
    for f in fields {
        let n = match f.value {
            Value::Int(i) => Some(*i as f64),
            Value::Uint(u) => Some(*u as f64),
            Value::Float(v) if v.is_nan() || v.is_infinite() => None,
            Value::Float(v) => Some(*v),
            _ => None,
        };
        if let Some(n) = n {
            if let Some(line) = render_line(&cfg.prefix, f, n, ts) {
                out.push_str(&line);
            }
        }
    }
    out
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let body = build_body(fields, cfg, now_secs());
    if body.is_empty() {
        return Ok(());
    }
    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter
        .next()
        .ok_or_else(|| GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host)))?;
    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_write_timeout(Some(timeout))?;
    s.write_all(body.as_bytes())?;
    s.flush()?;
    Ok(())
}
