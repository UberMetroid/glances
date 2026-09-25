//! Stdout JSON streamer: one object per tick.
//!
//! Each line is `{"timestamp":…,"plugins":{…}}` with the timestamp
//! first, flushed immediately for downstream pipelines. Non-finite
//! floats render null via the shared serializer.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::stats::GlancesStats;
use crate::core::value::Value;

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// The envelope value: timestamp plus the raw snapshot.
pub fn render_envelope(snapshot: &Value, timestamp: f64) -> Value {
    Value::Object(
        [("timestamp".to_string(), Value::Float(timestamp)), ("plugins".into(), snapshot.clone())]
            .into_iter()
            .collect(),
    )
}

/// One JSON line (no trailing newline), timestamp always first.
pub fn render_line(snapshot: &Value, timestamp: f64) -> String {
    crate::core::value::to_json_object_ordered(&[
        ("timestamp".to_string(), Value::Float(timestamp)),
        ("plugins".to_string(), snapshot.clone()),
    ])
}

/// The JSON loop: update, filter to the requested plugins, write one
/// line, flush. Stops after `stop_after` ticks when set; sleeps the
/// refresh gap between ticks (skipped when non-positive). Failed
/// updates warn and reuse stale data.
pub fn run(stats: &GlancesStats, refresh_secs: f32, stop_after: Option<u32>, args: &crate::cli::args::Args) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut tick: u32 = 0;
    loop {
        if let Err(e) = stats.update() {
            crate::core::logger::warning(&format!("json_stdout: stats.update() failed: {e}"));
        }
        let snap = crate::outputs::filter::filter_plugins(
            &super::csv_stdout::collect_snapshot(stats),
            &args.stdout_plugins,
        );
        let _ = writeln!(out, "{}", render_line(&snap, now_secs()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn obj(pairs: &[(&str, Value)]) -> Value {
        Value::Object(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect::<BTreeMap<_, _>>())
    }

    #[test]
    fn line_pins_timestamp_first() {
        let line = render_line(&obj(&[("cpu", obj(&[("total", Value::Float(1.5))]))]), 1.25);
        assert!(line.starts_with("{\"timestamp\":1.25,\"plugins\":{"));
        assert!(line.contains("\"cpu\":{\"total\":1.5}"));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn non_finite_floats_render_null() {
        let line = render_line(&obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]), 0.0);
        assert!(line.contains("\"bad\":null"));
    }

    #[test]
    fn empty_snapshot_stays_an_object() {
        let line = render_line(&Value::Object(BTreeMap::new()), 1.0);
        assert_eq!(line, "{\"timestamp\":1.0,\"plugins\":{}}");
    }
}
