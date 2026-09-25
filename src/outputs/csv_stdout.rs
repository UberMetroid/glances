//! Stdout CSV streamer: one row per stat per tick.
//!
//! Long-form rows — `timestamp,plugin,key,value,,` (the unit and
//! description columns stay empty) — with the header on the first
//! tick. Floats print at 2 decimals; non-finite floats print as
//! `NaN`/`Inf`/`-Inf` so parsers can flag them without breaking rows.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::stats::GlancesStats;
use crate::core::value::Value;

/// Header line, emitted once before the first tick's rows.
pub const HEADER: &str = "timestamp,plugin,key,value,unit,description";

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Quote a field when it contains `,`/`"`/newlines (quotes doubled).
pub fn csv_escape(s: &str) -> String {
    if s.chars().any(|c| matches!(c, ',' | '"' | '\n' | '\r')) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// One value as a cell string.
pub fn value_to_cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) if f.is_nan() => "NaN".into(),
        Value::Float(f) if *f == f64::INFINITY => "Inf".into(),
        Value::Float(f) if *f == f64::NEG_INFINITY => "-Inf".into(),
        Value::Float(f) => format!("{f:.2}"),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => crate::core::value::to_json(v),
    }
}

/// One tick's rows (no trailing newlines): objects flatten one row per
/// key, arrays one row per element, anything else one keyless row.
pub fn render_rows(snapshot: &Value, timestamp: f64) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(plugins) = snapshot.as_object() {
        for (name, stats) in plugins {
            render_plugin(name, stats, timestamp, &mut out);
        }
    }
    out
}

fn row(ts: f64, plugin: &str, key: &str, cell: &str) -> String {
    format!("{ts:.3},{},{},{},,", csv_escape(plugin), csv_escape(key), csv_escape(cell))
}

fn render_plugin(name: &str, v: &Value, ts: f64, out: &mut Vec<String>) {
    match v {
        Value::Object(fields) => {
            for (key, val) in fields {
                out.push(row(ts, name, key, &value_to_cell(val)));
            }
        }
        Value::Array(items) => {
            for item in items {
                render_element(name, item, ts, out);
            }
        }
        scalar => out.push(row(ts, name, "", &value_to_cell(scalar))),
    }
}

/// Array elements key off their `key` field with the rest JSON-encoded
/// (full shape preserved); non-objects render keyless.
fn render_element(plugin: &str, item: &Value, ts: f64, out: &mut Vec<String>) {
    match item.as_object() {
        Some(obj) => {
            let ident = obj.get("key").map(value_to_cell).unwrap_or_default();
            let rest: BTreeMap<String, Value> =
                obj.iter().filter(|(k, _)| k.as_str() != "key").map(|(k, v)| (k.clone(), v.clone())).collect();
            let body = crate::core::value::to_json(&Value::Object(rest));
            out.push(row(ts, plugin, &ident, &body));
        }
        None => out.push(row(ts, plugin, "", &value_to_cell(item))),
    }
}

/// The CSV loop: update, filter to the requested plugins, write rows,
/// flush. Header on the first tick only. Stops after `stop_after`
/// ticks when set; sleeps the refresh gap between ticks (skipped when
/// non-positive). Failed updates warn and reuse stale data.
pub fn run(stats: &GlancesStats, refresh_secs: f32, stop_after: Option<u32>, args: &crate::cli::args::Args) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut tick: u32 = 0;
    let mut headed = false;
    loop {
        if let Err(e) = stats.update() {
            crate::core::logger::warning(&format!("csv_stdout: stats.update() failed: {e}"));
        }
        let snap = crate::outputs::filter::filter_plugins(&collect_snapshot(stats), &args.stdout_plugins);
        if !headed {
            let _ = writeln!(out, "{HEADER}");
            headed = true;
        }
        for line in render_rows(&snap, now_secs()) {
            let _ = writeln!(out, "{line}");
        }
        let _ = out.flush();
        tick = tick.saturating_add(1);
        if stop_after.is_some_and(|max| tick >= max) {
            break;
        }
        if refresh_secs > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f32(refresh_secs));
        }
    }
}

/// Current name → stats map across all registered plugins.
pub fn collect_snapshot(stats: &GlancesStats) -> Value {
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    Value::Object(guard.iter().map(|p| (p.name().to_string(), p.stats().clone())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn obj(pairs: &[(&str, Value)]) -> Value {
        Value::Object(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect::<BTreeMap<_, _>>())
    }

    #[test]
    fn quoting_triggers_and_doubles() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_escape("a\nb"), "\"a\nb\"");
        assert_eq!(csv_escape("a\rb"), "\"a\rb\"");
    }

    // 3.14159 is an arbitrary rounding fixture, not PI.
    #[allow(clippy::approx_constant)]
    #[test]
    fn cells_cover_every_shape() {
        assert_eq!(value_to_cell(&Value::Float(3.14159)), "3.14");
        assert_eq!(value_to_cell(&Value::Float(0.0)), "0.00");
        assert_eq!(value_to_cell(&Value::Float(f64::NAN)), "NaN");
        assert_eq!(value_to_cell(&Value::Float(f64::INFINITY)), "Inf");
        assert_eq!(value_to_cell(&Value::Float(f64::NEG_INFINITY)), "-Inf");
        assert_eq!(value_to_cell(&Value::Null), "");
        assert_eq!(value_to_cell(&Value::Bool(true)), "true");
    }

    #[test]
    fn objects_flatten_one_row_per_key() {
        let snap = obj(&[("cpu", obj(&[("total", Value::Float(50.0)), ("idle", Value::Float(50.0))]))]);
        let rows = render_rows(&snap, 1.0);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].starts_with("1.000,cpu,idle,50.00,,"));
        assert!(rows[1].starts_with("1.000,cpu,total,50.00,,"));
    }

    #[test]
    fn array_elements_key_and_carry_json() {
        let mut e = BTreeMap::new();
        e.insert("key".into(), Value::String("eth0".into()));
        e.insert("rx".into(), Value::Float(100.5));
        let rows = render_rows(&obj(&[("network", Value::Array(vec![Value::Object(e)]))]), 2.0);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("network") && rows[0].contains("eth0"));
    }
}
