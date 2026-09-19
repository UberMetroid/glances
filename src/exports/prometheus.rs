//! Prometheus text-exposition exporter — writes to stdout.
//!
//! Per refresh tick, emits one full snapshot using the text exposition
//! format (`# HELP`, `# TYPE`, `metric value timestamp`). NaN/Inf samples
//! are emitted as `NaN` per Prometheus convention; non-numeric values
//! are skipped.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::Result;
use crate::core::value::Value;

pub const NAME: &str = "prometheus";

#[derive(Debug, Clone)]
pub struct Config {
    /// Namespace prefix prepended to every metric. Default `glances`.
    pub prefix: String,
    /// Include the unix-timestamp suffix on each sample. Default true.
    pub include_timestamp: bool,
    /// Optional override for the timestamp (seconds since epoch).
    pub timestamp: Option<f64>,
}

/// In-memory capture buffer for tests. `into_string()` returns the
/// accumulated bytes as a UTF-8 string (lossy for non-UTF-8 input).
#[derive(Clone)]
pub struct WriterSink {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl WriterSink {
    pub fn stdout() -> Self {
        // For stdout we still need a real Writer — see write_to for
        // the branching path. We return a sink that points to /dev/null
        // so callers that ignore the return value see no test pollution.
        Self { inner: Arc::new(Mutex::new(Vec::new())) }
    }
    pub fn in_memory() -> Self {
        Self { inner: Arc::new(Mutex::new(Vec::new())) }
    }
    pub fn into_string(self) -> String {
        let g = self.inner.lock().expect("sink poisoned");
        String::from_utf8_lossy(&g).into_owned()
    }
    fn as_writer(&self) -> SinkWriter {
        SinkWriter { inner: self.inner.clone() }
    }
}

struct SinkWriter {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl Write for SinkWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut g = self.inner.lock().expect("sink poisoned");
        g.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: "glances".into(),
            include_timestamp: true,
            timestamp: None,
        }
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Render the snapshot as Prometheus text exposition and write it to
/// stdout.
pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    let ts = cfg.timestamp.unwrap_or_else(now_secs);
    let ts_str = if cfg.include_timestamp { format!(" {}", ts as i64) } else { String::new() };
    let mut buf = Vec::<u8>::new();
    render(snap, cfg, &mut buf, &ts_str)?;
    let stdout = io::stdout();
    let mut g = stdout.lock();
    g.write_all(&buf)?;
    g.flush()?;
    Ok(())
}

/// Render the snapshot and write it to a custom sink (tests use this to
/// capture the output without touching real stdout).
pub fn write_to(snap: &Value, cfg: &Config, sink: &WriterSink) -> Result<()> {
    let ts = cfg.timestamp.unwrap_or_else(now_secs);
    let ts_str = if cfg.include_timestamp { format!(" {}", ts as i64) } else { String::new() };

    let mut buf = Vec::<u8>::new();
    render(snap, cfg, &mut buf, &ts_str).map_err(io::Error::from)?;

    let mut w = sink.as_writer();
    w.write_all(&buf)?;
    w.flush()?;
    Ok(())
}

fn render(snap: &Value, cfg: &Config, buf: &mut Vec<u8>, ts_suffix: &str) -> io::Result<()> {
    let plugins = match snap.as_object() {
        Some(o) => o,
        None => return Ok(()),
    };
    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        for (key, val) in fields {
            let metric = format!("{}_{}_{}", cfg.prefix, sanitize(plugin), sanitize(key));
            let repr = match val {
                Value::Int(i) => i.to_string(),
                Value::Uint(u) => u.to_string(),
                Value::Float(f) if f.is_nan() => "NaN".into(),
                Value::Float(f) if f.is_infinite() => {
                    if *f > 0.0 { "+Inf".into() } else { "-Inf".into() }
                }
                Value::Float(f) => format!("{}", f),
                _ => continue,
            };
            writeln!(buf, "# HELP {} {} {}", metric, plugin, key)?;
            writeln!(buf, "# TYPE {} gauge", metric)?;
            writeln!(buf, "{} {}{}", metric, repr, ts_suffix)?;
        }
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' | '-' | '/' | '.' | ':' => '_',
            c if c.is_ascii_alphanumeric() || c == '_' => c,
            other => other,
        })
        .collect()
}