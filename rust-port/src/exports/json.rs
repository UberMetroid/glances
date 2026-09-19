//! JSON exporter — append a one-line JSON envelope per refresh tick.
//!
//! Each line is `{"timestamp": <secs>, "stats": <snap>}\n`. NaN/Inf values
//! inside `stats` are rendered as JSON `null` by `core::value::to_json`.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::{GlancesError, Result};
use crate::core::value::{to_json, Value};

pub const NAME: &str = "json";

#[derive(Debug, Clone)]
pub struct Config {
    pub path: String,
    /// Optional override for the timestamp (seconds since epoch).
    pub timestamp: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Self { path: String::new(), timestamp: None }
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Append one JSON line for `snap` to `cfg.path`.
pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.path.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "json exporter requires a non-empty path".into(),
        ));
    }
    let ts = cfg.timestamp.unwrap_or_else(now_secs);

    let mut envelope = BTreeMap::new();
    envelope.insert("timestamp".to_string(), Value::Float(ts));
    envelope.insert("stats".to_string(), snap.clone());
    let envelope = Value::Object(envelope);

    let line = format!("{}\n", to_json(&envelope));

    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&cfg.path)?;
    f.write_all(line.as_bytes())?;
    f.flush()?;
    Ok(())
}