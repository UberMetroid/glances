//! StatsD exporter — UDP packet-per-metric.
//!
//! Format: `glances.<series>.<key>:<value>|<type>\n` where `series` is
//! `plugin` or `plugin.<elem>` for array plugins.
//!   - `g` for gauges (default, incl. bools rendered 1/0)
//!   - `c` for counters (keys ending in `_count` or `_total`)
//!
//! NaN / Inf values are skipped so the receiving StatsD daemon does not
//! choke. UDP is fire-and-forget — no reconnect logic.

use std::net::UdpSocket;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::Result;
use crate::core::value::Value;
use crate::exports::flatten::Field;

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

/// Send one StatsD packet per field to `host:port`. The `(host, port)`
/// tuple form keeps bare IPv6 literals connectable (`v6:addr:port`
/// string-joins would mangle them).
pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let _ts = cfg.timestamp.unwrap_or_else(|| now_ms() as f64);
    let socket = UdpSocket::bind("0.0.0.0:0")?;

    for f in fields {
        if let Some(packet) = render(&f.series, f.key, f.value) {
            socket.send_to(packet.as_bytes(), (cfg.host.as_str(), cfg.port))?;
        }
    }
    Ok(())
}

fn render(series: &str, key: &str, v: &Value) -> Option<String> {
    let metric_name = format!("glances.{}.{}", sanitize(series), sanitize(key));
    let packet = match v {
        Value::Int(i) => format!("{}:{}|{}", metric_name, i, type_for(key)),
        Value::Uint(u) => format!("{}:{}|{}", metric_name, u, type_for(key)),
        Value::Float(f) if f.is_nan() || f.is_infinite() => return None,
        Value::Float(f) => format!("{}:{}|{}", metric_name, f, type_for(key)),
        // Bools are state, not events — emit a gauge 1/0, not a counter.
        Value::Bool(b) => format!("{}:{}|g", metric_name, if *b { 1 } else { 0 }),
        _ => return None,
    };
    Some(format!("{}\n", packet))
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
