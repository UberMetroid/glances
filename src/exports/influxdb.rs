//! InfluxDB v1.x line-protocol exporter over TCP.
//!
//! Per refresh tick, open a fresh TCP connection to `host:port`, write
//! `measurement,tag=value field=value timestamp_ns\n` lines, and close.
//! On failure, sleep 5s and try once more before returning the error.

use std::io::Write;
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "influxdb";

const BACKOFF_SECS: u64 = 5;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// Optional override for the timestamp (seconds since epoch).
    pub timestamp: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Self { host: "127.0.0.1".into(), port: 8086, timestamp: None }
    }
}

fn now_nanos() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Send the snapshot as InfluxDB line-protocol over TCP. Reconnects once
/// with a 5s backoff on connection / write failure.
pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    let body = format_lines(snap, cfg.timestamp);
    if body.is_empty() { return Ok(()); }

    let addr = format!("{}:{}", cfg.host, cfg.port);
    let result = try_send(&addr, &body);
    match result {
        Ok(()) => Ok(()),
        Err(first_err) => {
            eprintln!(
                "[{}] first attempt failed: {}; backing off {}s",
                NAME, first_err, BACKOFF_SECS
            );
            std::thread::sleep(std::time::Duration::from_secs(BACKOFF_SECS));
            try_send(&addr, &body).map_err(|e| {
                GlancesError::Other(format!(
                    "{} export failed after retry: {} (first: {})",
                    NAME, e, first_err
                ))
            })
        }
    }
}

fn try_send(addr: &str, body: &str) -> Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn format_lines(snap: &Value, ts_override: Option<f64>) -> String {
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
            if let Some(s) = field_to_lp(k, v) {
                field_strs.push(s);
            }
        }
        // Emit a header line even when all fields are NaN/Inf — this
        // preserves the plugin's presence in the InfluxDB series (e.g.
        // a sensor that briefly reads as NaN still shows up at the
        // expected timestamp).
        out.push_str(&escape_measurement(plugin));
        out.push(' ');
        out.push_str(&field_strs.join(","));
        out.push(' ');
        out.push_str(&ts.to_string());
        out.push('\n');
    }
    out
}

fn field_to_lp(key: &str, v: &Value) -> Option<String> {
    let k = escape_tag(key);
    let rendered = match v {
        Value::Int(i) => i.to_string() + "i",
        Value::Uint(u) => u.to_string() + "i",
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