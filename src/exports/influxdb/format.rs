//! InfluxDB line-protocol rendering (upstream normalize parity).

use std::time::{SystemTime, UNIX_EPOCH};

use super::Config;
use super::super::flatten::Field;
use crate::core::value::Value;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Fields upstream promotes to tags (`FIELD_TO_TAG` parity).
const FIELD_TO_TAG: &[&str] = &["name", "cmdline", "type"];

/// Parse upstream `tags` config form (`key:value` pairs, comma separated).
pub fn parse_tags(s: &str) -> Vec<(String, String)> {
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

/// Group flat fields into `measurement[,tags] field=v,… ts` lines
/// (upstream `normalize_for_influxdb` parity):
/// - measurement = plugin (element identity goes in tags, not the name)
/// - tags = configured tags + `hostname` + key-promotion + FIELD_TO_TAG
/// - numbers → float fields, strings stay strings, nulls skipped
/// - second-precision timestamps (`time_precision="s"` parity).
pub(crate) fn format_lines(fields: &[Field<'_>], cfg: &Config, ts_override: Option<f64>) -> String {
    let ts = match ts_override {
        Some(s) => s as i64,
        None => now_secs(),
    };
    // Group consecutive (plugin, elem) fields into one LP line.
    let mut out = String::new();
    let mut cur_plugin = String::new();
    let mut cur_elem: Option<String> = None;
    let mut cur_key_field: Option<String> = None;
    let mut cur_fields: Vec<String> = Vec::new();
    let mut cur_tags: Vec<(String, String)> = Vec::new();
    let mut started = false;
    let flush = |plugin: &str, elem: &Option<String>, kf: &Option<String>,
                     fs: &mut Vec<String>, tags: &mut Vec<(String, String)>,
                     out: &mut String| {
        if fs.is_empty() { return; } // no fields → invalid LP line; skip
        let mut measurement = escape_measurement(plugin);
        if !cfg.prefix.is_empty() {
            measurement = format!("{}.{}", escape_measurement(&cfg.prefix), measurement);
        }
        out.push_str(&measurement);
        let mut all_tags: Vec<String> = cfg
            .tags
            .iter()
            .map(|(k, v)| format!("{}={}", escape_tag(k), escape_tag(v)))
            .collect();
        if !cfg.hostname.is_empty() {
            all_tags.push(format!("hostname={}", escape_tag(&cfg.hostname)));
        }
        if let (Some(k), Some(e)) = (kf.as_ref(), elem.as_ref()) {
            all_tags.push(format!("{}={}", escape_tag(k), escape_tag(e)));
        }
        for (k, v) in tags.drain(..) {
            all_tags.push(format!("{}={}", escape_tag(&k), escape_tag(&v)));
        }
        if !all_tags.is_empty() {
            out.push(',');
            out.push_str(&all_tags.join(","));
        }
        out.push(' ');
        out.push_str(&fs.join(","));
        out.push(' ');
        out.push_str(&ts.to_string());
        out.push('\n');
        fs.clear();
    };
    for f in fields {
        if started && (f.plugin != cur_plugin.as_str() || f.elem != cur_elem) {
            flush(&cur_plugin, &cur_elem, &cur_key_field, &mut cur_fields, &mut cur_tags, &mut out);
        }
        started = true;
        cur_plugin = f.plugin.to_owned();
        cur_elem = f.elem.clone();
        cur_key_field = f.key_field.map(|s| s.to_string());
        if FIELD_TO_TAG.contains(&f.key) {
            if let Some(s) = scalar_string(f.value) {
                cur_tags.push((f.key.to_string(), s));
            }
            continue;
        }
        if f.key == "result" {
            // Upstream #3419: keep the string AND a numeric twin.
            if let Some(s) = scalar_string(f.value) {
                cur_fields.push(format!("{}=\"{}\"", escape_tag(f.key), escape_field_str(&s)));
            }
            if let Some(n) = scalar_float(f.value) {
                cur_fields.push(format!("{}_float={}", escape_tag(f.key), n));
            }
            continue;
        }
        if let Some(s) = field_to_lp(f.key, f.value) {
            cur_fields.push(s);
        }
    }
    if started {
        flush(&cur_plugin, &cur_elem, &cur_key_field, &mut cur_fields, &mut cur_tags, &mut out);
    }
    out
}

/// Best-effort float (`float(v)` parity); `None` for non-numerics.
fn scalar_float(v: &Value) -> Option<String> {
    match v {
        Value::Int(i) => Some(format!("{}.0", i)),
        Value::Uint(u) => Some(format!("{}.0", u)),
        Value::Float(f) if f.is_nan() || f.is_infinite() => None,
        Value::Float(f) => Some(format!("{}", f)),
        Value::Bool(b) => Some(if *b { "1.0".into() } else { "0.0".into() }),
        _ => None,
    }
}

/// Best-effort string (`str(v)` parity); `None` for null/containers.
fn scalar_string(v: &Value) -> Option<String> {
    match v {
        Value::Null | Value::Array(_) | Value::Object(_) => None,
        Value::String(s) => Some(s.clone()),
        Value::Int(i) => Some(i.to_string()),
        Value::Uint(u) => Some(u.to_string()),
        Value::Float(f) if f.is_nan() || f.is_infinite() => None,
        Value::Float(f) => Some(format!("{}", f)),
        Value::Bool(b) => Some(b.to_string()),
    }
}

fn field_to_lp(key: &str, v: &Value) -> Option<String> {
    let k = escape_tag(key);
    let rendered = match v {
        // Upstream converts every number with float(): LP floats.
        Value::Int(i) => format!("{}.0", i),
        Value::Uint(u) => format!("{}.0", u),
        Value::Float(f) if f.is_nan() || f.is_infinite() => return None,
        Value::Float(f) => format!("{}", f),
        Value::Bool(b) => {
            if *b { "1.0".into() } else { "0.0".into() }
        }
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

