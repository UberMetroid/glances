//! StatsD exporter — UDP packet-per-metric.
//!
//! Format: `glances.<plugin>.<key>:<value>|<type>\n`
//!   - `g` for gauges (default for numeric values)
//!   - `c` for counters (used when the key ends in `_count` or `_total`)
//!
//! NaN / Inf values are skipped so the receiving StatsD daemon does not
//! choke. UDP is fire-and-forget — no reconnect logic.

use std::net::UdpSocket;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "statsd";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// Optional override for the timestamp (milliseconds since epoch).
    /// StatsD itself is time-agnostic; we keep this only for symmetry.
    pub timestamp: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Self { host: "127.0.0.1".into(), port: 8125, timestamp: None }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Send one StatsD packet per `(plugin, key)` to `host:port`.
pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    let _ts = cfg.timestamp.unwrap_or_else(|| now_ms() as f64);
    let addr = format!("{}:{}", cfg.host, cfg.port);
    let socket = UdpSocket::bind("0.0.0.0:0")?;

    let plugins = snap
        .as_object()
        .ok_or_else(|| GlancesError::Parse("snapshot must be a JSON object".into()))?;

    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        for (key, v) in fields {
            if let Some(packet) = render(plugin, key, v) {
                socket.send_to(packet.as_bytes(), &addr)?;
            }
        }
    }
    Ok(())
}

fn render(plugin: &str, key: &str, v: &Value) -> Option<String> {
    let metric_name = format!("glances.{}.{}", sanitize(plugin), sanitize(key));
    match v {
        Value::Int(i) => Some(format!("{}:{}|{}", metric_name, i, type_for(key))),
        Value::Uint(u) => Some(format!("{}:{}|{}", metric_name, u, type_for(key))),
        Value::Float(f) if f.is_nan() || f.is_infinite() => None,
        Value::Float(f) => Some(format!("{}:{}|{}", metric_name, f, type_for(key))),
        Value::Bool(b) => {
            let n = if *b { 1 } else { 0 };
            Some(format!("{}:{}|c", metric_name, n))
        }
        _ => None,
    }
}

fn type_for(key: &str) -> &'static str {
    if key.ends_with("_count") || key.ends_with("_total") { "c" } else { "g" }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ':' | '|' | '@' => '_',
            c if c.is_ascii_whitespace() => '_',
            c => c,
        })
        .collect()
}