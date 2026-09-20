//! Prometheus exposition rendering (labels + sanitizing).

use std::io::Write;

use super::Config;
use super::super::flatten::Field;
use crate::core::value::Value;

/// Parse upstream `labels` config form (`key:value` pairs, comma
/// separated) into a label list.
pub fn parse_labels(s: &str) -> Vec<(String, String)> {
    s.split(',')
        .filter_map(|pair| {
            let mut it = pair.splitn(2, ':');
            let k = it.next()?.trim();
            let v = it.next()?.trim();
            if k.is_empty() || v.is_empty() { return None; }
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Render into a provided buffer (tests capture without touching state).
/// Upstream parity: every sample carries the configured labels plus the
/// per-element identity label (`<key_field>="<elem>"`, e.g.
/// `interface_name="eth0"`) for list plugins.
pub fn render(fields: &[Field<'_>], cfg: &Config, buf: &mut Vec<u8>, ts_suffix: &str) -> std::io::Result<()> {
    for f in fields {
        // Metric name is plugin-level (series element goes in labels —
        // one series per (plugin, key), not per device).
        let metric = format!("{}_{}_{}", cfg.prefix, sanitize(&f.plugin), sanitize(f.key));
        let repr = match f.value {
            // Upstream converts every number with float(): exposition floats.
            Value::Int(i) => format!("{}.0", i),
            Value::Uint(u) => format!("{}.0", u),
            Value::Bool(b) => {
                if *b { "1.0".into() } else { "0.0".into() }
            }
            Value::Float(f) if f.is_nan() => "NaN".into(),
            Value::Float(f) if f.is_infinite() => {
                if *f > 0.0 { "+Inf".into() } else { "-Inf".into() }
            }
            Value::Float(f) => format!("{}", f),
            _ => continue,
        };
        let mut labels: Vec<String> = cfg
            .labels
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", sanitize(k), label_escape(v)))
            .collect();
        if let (Some(kf), Some(elem)) = (f.key_field, f.elem.as_ref()) {
            labels.push(format!("{}=\"{}\"", sanitize(kf), label_escape(elem)));
        }
        let help = format!("{}.{}", f.series, f.key);
        writeln!(buf, "# HELP {} {}", metric, help_escape(&help))?;
        writeln!(buf, "# TYPE {} gauge", metric)?;
        writeln!(buf, "{}{{{}}} {}{}", metric, labels.join(","), repr, ts_suffix)?;
    }
    Ok(())
}

/// Label-value escaping: `\` → `\\`, `"` → `\"`, newline → `\n`.
fn label_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// HELP text escaping: `\\` → `\\\\`, `\n` → `\\n` per exposition spec.
pub fn help_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

/// Metric-name charset: `[a-zA-Z0-9_:]`; anything else → `_`.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == ':' { c } else { '_' })
        .collect()
}

