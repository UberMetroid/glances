//! CSV exporter — append one CSV row per `(plugin, key)` pair per refresh tick.
//!
//! Format (RFC 4180-ish; quoted fields escape inner `"` as `""`):
//! ```text
//! timestamp,plugin,key,value,unit,description
//! ```
//!
//! Unit and description columns are emitted as empty strings; the snapshot
//! `Value` does not carry per-field metadata. NaN/Inf values render as the
//! literal string `NaN` so downstream parsers can flag them.

use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "csv";

#[derive(Debug, Clone)]
pub struct Config {
    /// Destination file path. Appended to; created if missing.
    pub path: String,
    /// Emit the header row when the file is created. Default true.
    pub write_header: bool,
    /// Optional override for the timestamp (seconds since epoch). When
    /// `None`, the exporter uses `SystemTime::now()`.
    pub timestamp: Option<f64>,
    /// Truncate instead of appending (`--export-csv-overwrite`).
    /// Default false (append, matching upstream without the flag).
    pub overwrite: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            path: String::new(),
            write_header: true,
            timestamp: None,
            overwrite: false,
        }
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Append CSV rows for every flattened field to `cfg.path`. Array-plugin
/// elements land in the `plugin` column as `plugin.elem`.
pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    if cfg.path.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "csv exporter requires a non-empty path".into(),
        ));
    }
    let ts = cfg.timestamp.unwrap_or_else(now_secs);
    let file_exists = std::path::Path::new(&cfg.path).exists() && !cfg.overwrite;

    let mut f = OpenOptions::new()
        .create(true)
        .append(!cfg.overwrite)
        .truncate(cfg.overwrite)
        .open(&cfg.path)?;

    if cfg.write_header && !file_exists {
        f.write_all(b"timestamp,plugin,key,value,unit,description\n")?;
    }

    for field in fields {
        let line = format!(
            "{},{},{},{},{unit},{desc}\n",
            ts,
            csv_escape(&field.series),
            csv_escape(field.key),
            csv_escape(&value_to_string(field.value)),
            unit = "",
            desc = "",
        );
        f.write_all(line.as_bytes())?;
    }
    f.flush()?;
    Ok(())
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) if f.is_nan() => "NaN".into(),
        Value::Float(f) if f.is_infinite() => {
            if *f > 0.0 { "Inf".into() } else { "-Inf".into() }
        }
        Value::Float(f) => format!("{}", f),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => v_json_like(v),
    }
}

fn v_json_like(v: &Value) -> String {
    let mut buf = String::new();
    write_value(v, &mut buf);
    buf
}

fn write_value(v: &Value, buf: &mut String) {
    use std::fmt::Write as _;
    match v {
        Value::Null => buf.push_str("null"),
        Value::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => { let _ = write!(buf, "{}", i); }
        Value::Uint(u) => { let _ = write!(buf, "{}", u); }
        Value::Float(f) if f.is_nan() || f.is_infinite() => buf.push_str("null"),
        Value::Float(f) => { let _ = write!(buf, "{}", f); }
        Value::String(s) => {
            // JSON-escape quotes/backslashes/control chars — bare inner
            // quotes made nested objects invalid JSON.
            buf.push('"');
            for c in s.chars() {
                match c {
                    '"' => buf.push_str("\\\""),
                    '\\' => buf.push_str("\\\\"),
                    '\n' => buf.push_str("\\n"),
                    '\r' => buf.push_str("\\r"),
                    '\t' => buf.push_str("\\t"),
                    c if c < ' ' => { let _ = write!(buf, "\\u{:04x}", c as u32); }
                    c => buf.push(c),
                }
            }
            buf.push('"');
        }
        Value::Array(arr) => {
            buf.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 { buf.push(','); }
                write_value(item, buf);
            }
            buf.push(']');
        }
        Value::Object(obj) => {
            buf.push('{');
            for (i, (k, val)) in obj.iter().enumerate() {
                if i > 0 { buf.push(','); }
                let _ = write!(buf, "\"{}\":", k);
                write_value(val, buf);
            }
            buf.push('}');
        }
    }
}

fn csv_escape(s: &str) -> String {
    let needs_quote = s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r');
    if !needs_quote { return s.to_string(); }
    let escaped = s.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}