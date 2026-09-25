//! `--stdout <spec>` output — print selected plugin attributes per tick.
//!
//! The spec is a comma-separated list of `plugin` or `plugin.attr`
//! selectors (Python Glances `--stdout` semantics): each refresh tick
//! prints one `name: value` line per resolved attribute, in spec order.

use std::io::Write;

use crate::core::stats::GlancesStats;
use crate::core::value::Value;

/// Parse `"cpu.total,mem,load.min1"` into selector pairs.
/// `attr` is `None` for whole-plugin selectors.
pub fn parse_spec(spec: &str) -> Vec<(String, Option<String>)> {
    spec.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| match s.split_once('.') {
            Some((p, a)) => (p.to_string(), Some(a.to_string())),
            None => (s.to_string(), None),
        })
        .collect()
}

/// Resolve one selector against the snapshot into rendered lines.
/// A bare `plugin` selector emits every leaf `plugin.attr` in key order.
fn render_selector(snap: &Value, plugin: &str, attr: Option<&str>) -> Vec<String> {
    let plugin_val = snap.as_object().and_then(|o| o.get(plugin));
    let mut out = Vec::new();
    match (plugin_val, attr) {
        (Some(Value::Object(fields)), Some(a)) => {
            if let Some(v) = fields.get(a) {
                out.push(format!("{}.{}: {}", plugin, a, cell(v)));
            }
        }
        (Some(Value::Object(fields)), None) => {
            for (k, v) in fields {
                out.push(format!("{}.{}: {}", plugin, k, cell(v)));
            }
        }
        (Some(v), None) => out.push(format!("{}: {}", plugin, cell(v))),
        _ => {}
    }
    out
}

fn cell(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) => format!("{:.2}", f),
        Value::String(s) => s.clone(),
        _ => crate::core::value::to_json(v),
    }
}

/// Drive the stdout loop. Same tick semantics as `csv_stdout::run`.
pub fn run(stats: &GlancesStats, spec: &str, refresh_secs: f32, stop_after: Option<u32>) {
    let selectors = parse_spec(spec);
    if selectors.is_empty() {
        crate::core::logger::warning("--stdout: empty spec, nothing to print");
        return;
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut tick: u32 = 0;
    let mut warned_empty = false;
    loop {
        if let Err(e) = stats.update() {
            crate::core::logger::warning(&format!("stdout: stats.update() failed: {}", e));
        }
        let snap = stats.snapshot();
        let mut lines_emitted = 0usize;
        for (plugin, attr) in &selectors {
            for line in render_selector(&snap, plugin, attr.as_deref()) {
                lines_emitted += 1;
                let _ = writeln!(out, "{}", line);
            }
        }
        // A spec that resolves to nothing (typo'd plugin/attr name)
        // would otherwise loop forever printing silence.
        if lines_emitted == 0 && !warned_empty {
            crate::core::logger::warning(&format!(
                "--stdout: spec '{}' matched no plugin attributes",
                spec
            ));
            warned_empty = true;
        }
        let _ = out.flush();
        tick = tick.saturating_add(1);
        if let Some(max) = stop_after
            && tick >= max { break; }
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
    fn parse_spec_splits_plugin_and_attr() {
        assert_eq!(
            parse_spec("cpu.total, mem , load.min1"),
            vec![
                ("cpu".to_string(), Some("total".to_string())),
                ("mem".to_string(), None),
                ("load".to_string(), Some("min1".to_string())),
            ]
        );
    }

    #[test]
    fn render_selector_attr_and_plugin() {
        let snap = obj(&[("cpu", obj(&[("total", Value::Float(12.34)), ("idle", Value::Float(87.0))]))]);
        assert_eq!(render_selector(&snap, "cpu", Some("total")), vec!["cpu.total: 12.34"]);
        assert_eq!(
            render_selector(&snap, "cpu", None),
            vec!["cpu.idle: 87.00", "cpu.total: 12.34"]
        );
        assert!(render_selector(&snap, "nope", None).is_empty());
        assert!(render_selector(&snap, "cpu", Some("nope")).is_empty());
    }
}
