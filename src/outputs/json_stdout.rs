//! M12 — Stdout JSON streamer.
//!
//! One JSON object per refresh tick, written to stdout and flushed
//! immediately so downstream pipelines (`jq`, `curl`-style consumers,
//! fluentd/vector, etc.) see fresh data. Shape:
//!
//! ```json
//! {"timestamp": 1700000000.123, "cpu": {"total": 12.5, ...}, "mem": {...}}
//! ```
//!
//! NaN / ±Infinity inside plugin stats render as JSON `null` per
//! `core::value::to_json`.
//!
//! Mirrors `glances/outputs/glances_stdout_json.py`.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::stats::GlancesStats;
use crate::core::value::Value;

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Build the envelope object `{ "timestamp": ..., "plugins": {...} }`
/// from a raw snapshot.
pub fn render_envelope(snapshot: &Value, timestamp: f64) -> Value {
    let mut obj = std::collections::BTreeMap::new();
    obj.insert("timestamp".into(), Value::Float(timestamp));
    obj.insert("plugins".into(), snapshot.clone());
    Value::Object(obj)
}

/// Render one JSON line (no trailing newline). The result is always a
/// single-line valid JSON object with timestamp before plugins (matching
/// Python Glances' JSON stdout format).
pub fn render_line(snapshot: &Value, timestamp: f64) -> String {
    use crate::core::value::to_json_object_ordered;
    to_json_object_ordered(&[
        ("timestamp".to_string(), Value::Float(timestamp)),
        ("plugins".to_string(), snapshot.clone()),
    ])
}

/// Drive the JSON stdout loop. Calls `stats.update()` once per tick,
/// writes one JSON line per tick to stdout, flushes after each line.
/// Sleeps `refresh_secs` between ticks. Returns after `stop_after`
/// ticks when set, otherwise loops forever.
pub fn run(stats: &GlancesStats, refresh_secs: f32, stop_after: Option<u32>, args: &crate::cli::args::Args) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut tick: u32 = 0;
    loop {
        if let Err(e) = stats.update() {
            crate::core::logger::warning(&format!("json_stdout: stats.update() failed: {}", e));
        }
        let snap = super::csv_stdout::collect_snapshot(stats);
        if !args.export_targets.is_empty() {
            crate::exports::write_targets(&snap, args);
        }
        let line = render_line(&snap, now_secs());
        let _ = writeln!(out, "{}", line);
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
    fn envelope_has_timestamp_and_plugins() {
        let snap = obj(&[("cpu", obj(&[("total", Value::Float(1.5))]))]);
        let line = render_line(&snap, 1.25);
        // to_json_object_ordered pins timestamp before plugins.
        assert!(line.starts_with("{\"timestamp\":1.25,\"plugins\":{"));
        assert!(line.contains("\"cpu\":{\"total\":1.5}"));
    }

    #[test]
    fn nan_becomes_null_in_output() {
        let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
        let line = render_line(&snap, 0.0);
        assert!(line.contains("\"bad\":null"));
    }

    #[test]
    fn line_is_single_line() {
        let snap = obj(&[("mem", obj(&[("used", Value::Uint(1024))]))]);
        let line = render_line(&snap, 0.0);
        assert!(!line.contains('\n'));
    }

    #[test]
    fn empty_snapshot_yields_empty_plugins_object() {
        let snap = Value::Object(BTreeMap::new());
        let line = render_line(&snap, 1.0);
        // to_json_object_ordered pins timestamp before plugins.
        assert_eq!(line, "{\"timestamp\":1.0,\"plugins\":{}}");
    }
}
