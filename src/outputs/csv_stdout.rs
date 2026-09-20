//! M12 — Stdout CSV streamer.
//!
//! Per refresh tick, emits one CSV row per plugin/key combination. The row
//! format is the long form:
//!
//! ```text
//! timestamp,plugin,key,value,unit,description
//! ```
//!
//! Header is emitted on the first tick. Floats are rendered with `{:.2}`
//! to match Python Glances' stdout CSV behaviour. NaN / ±Infinity are
//! rendered as the literal string `NaN` so downstream parsers can flag
//! them without breaking the row layout.
//!
//! Mirrors `glances/outputs/glances_stdout_csv.py`.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::stats::GlancesStats;
use crate::core::value::Value;

/// Header line emitted on the first tick. Exposed for testing.
pub const HEADER: &str = "timestamp,plugin,key,value,unit,description";

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Escape a CSV field per RFC 4180: a quote is doubled, and any field
/// containing `,`, `"`, `\n`, or `\r` is wrapped in `"..."`.
pub fn csv_escape(s: &str) -> String {
    let needs_quote = s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r');
    if !needs_quote {
        return s.to_string();
    }
    let escaped = s.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}

/// Render a single `Value` to a CSV cell string.
pub fn value_to_cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) if f.is_nan() => "NaN".into(),
        Value::Float(f) if f.is_infinite() => {
            if *f > 0.0 { "Inf".into() } else { "-Inf".into() }
        }
        Value::Float(f) => format!("{:.2}", f),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => crate::core::value::to_json(v),
    }
}

/// Build the rows for one tick. Returns a vector of fully-formatted CSV
/// lines (without trailing newline). First call should be paired with
/// emitting `HEADER` separately.
pub fn render_rows(snapshot: &Value, timestamp: f64) -> Vec<String> {
    let mut out = Vec::new();
    let plugins = match snapshot.as_object() {
        Some(o) => o,
        None => return out,
    };
    for (plugin_name, plugin_value) in plugins {
        render_plugin_rows(plugin_name, plugin_value, timestamp, &mut out);
    }
    out
}

fn render_plugin_rows(name: &str, v: &Value, ts: f64, out: &mut Vec<String>) {
    match v {
        Value::Object(fields) => {
            for (key, val) in fields {
                out.push(format!(
                    "{},{},{},{},,",
                    format!("{:.3}", ts),
                    csv_escape(name),
                    csv_escape(key),
                    csv_escape(&value_to_cell(val)),
                ));
            }
        }
        Value::Array(items) => {
            for item in items {
                render_array_row(name, item, ts, out);
            }
        }
        _ => {
            out.push(format!(
                "{},{},{},{},,",
                format!("{:.3}", ts),
                csv_escape(name),
                "",
                csv_escape(&value_to_cell(v)),
            ));
        }
    }
}

fn render_array_row(plugin_name: &str, item: &Value, ts: f64, out: &mut Vec<String>) {
    if let Some(obj) = item.as_object() {
        // Element's "key" field becomes the CSV `key` column; the rest is
        // JSON-encoded into `value` so callers retain the full shape.
        let ident = obj.get("key").map(value_to_cell).unwrap_or_default();
        let without_key: BTreeMap<String, Value> = obj
            .iter()
            .filter(|(k, _)| k.as_str() != "key")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let value_repr = crate::core::value::to_json(&Value::Object(without_key));
        out.push(format!(
            "{},{},{},{},,",
            format!("{:.3}", ts),
            csv_escape(plugin_name),
            csv_escape(&ident),
            csv_escape(&value_repr),
        ));
    } else {
        out.push(format!(
            "{},{},{},{},,",
            format!("{:.3}", ts),
            csv_escape(plugin_name),
            "",
            csv_escape(&value_to_cell(item)),
        ));
    }
}

/// Drive the CSV stdout loop. Calls `stats.update()` once per tick, then
/// writes the CSV header (first tick only) followed by one line per
/// `(plugin, key)` row. Sleeps `refresh_secs` between ticks. Returns
/// after `stop_after` ticks when set, otherwise loops forever.
pub fn run(stats: &GlancesStats, refresh_secs: f32, stop_after: Option<u32>, args: &crate::cli::args::Args) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut emitted_header = false;
    let mut tick: u32 = 0;
    loop {
        if let Err(e) = stats.update() {
            crate::core::logger::warning(&format!("csv_stdout: stats.update() failed: {}", e));
        }
        let snap = collect_snapshot(stats);
        if !args.export_targets.is_empty() {
            let keys = stats.plugin_keys();
            crate::exports::write_targets(&snap, args, &keys);
        }
        let ts = now_secs();
        if !emitted_header {
            let _ = writeln!(out, "{}", HEADER);
            emitted_header = true;
        }
        for row in render_rows(&snap, ts) {
            let _ = writeln!(out, "{}", row);
        }
        let _ = out.flush();
        tick = tick.saturating_add(1);
        if let Some(max) = stop_after {
            if tick >= max { break; }
        }
        if refresh_secs > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f32(refresh_secs));
        }
    }
}

/// Collect the current snapshot from all registered plugins. The snapshot
/// shape is `{ plugin_name: stats_value, ... }`.
pub fn collect_snapshot(stats: &GlancesStats) -> Value {
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut obj = BTreeMap::new();
    for p in guard.iter() {
        obj.insert(p.name().to_string(), p.stats().clone());
    }
    Value::Object(obj)
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
    fn escape_basic() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("with,comma"), "\"with,comma\"");
        assert_eq!(csv_escape("with\"quote"), "\"with\"\"quote\"");
    }

    #[test]
    fn escape_newlines() {
        assert_eq!(csv_escape("a\nb"), "\"a\nb\"");
        assert_eq!(csv_escape("a\rb"), "\"a\rb\"");
    }

    #[test]
    fn value_to_cell_floats_two_decimals() {
        assert_eq!(value_to_cell(&Value::Float(3.14159)), "3.14");
        assert_eq!(value_to_cell(&Value::Float(0.0)), "0.00");
        assert_eq!(value_to_cell(&Value::Float(-1.5)), "-1.50");
    }

    #[test]
    fn value_to_cell_special_floats() {
        assert_eq!(value_to_cell(&Value::Float(f64::NAN)), "NaN");
        assert_eq!(value_to_cell(&Value::Float(f64::INFINITY)), "Inf");
        assert_eq!(value_to_cell(&Value::Float(f64::NEG_INFINITY)), "-Inf");
    }

    #[test]
    fn render_rows_flattens_object() {
        let cpu = obj(&[("total", Value::Float(50.0)), ("idle", Value::Float(50.0))]);
        let snap = obj(&[("cpu", cpu)]);
        let rows = render_rows(&snap, 1.0);
        assert_eq!(rows.len(), 2);
        // BTreeMap sorts keys: "idle" < "total" alphabetically.
        assert!(rows[0].starts_with("1.000,cpu,idle,50.00,,"));
        assert!(rows[1].starts_with("1.000,cpu,total,50.00,,"));
    }

    #[test]
    fn render_rows_handles_array() {
        let mut e1 = BTreeMap::new();
        e1.insert("key".into(), Value::String("eth0".into()));
        e1.insert("rx".into(), Value::Float(100.5));
        let net = Value::Array(vec![Value::Object(e1)]);
        let snap = obj(&[("network", net)]);
        let rows = render_rows(&snap, 2.0);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("network"));
        assert!(rows[0].contains("eth0"));
    }
}
