//! Prometheus text-exposition exporter.
//!
//! Two sinks, mirroring Python Glances' prometheus exporter:
//!   - **Scrape listener**: an HTTP server on `cfg.port` serving the
//!     latest exposition at `GET /metrics` (started on first write).
//!   - **File**: `cfg.file` appends the exposition each tick.
//!
//! Timestamp suffixes are emitted in **milliseconds** per the exposition
//! spec. NaN/Inf samples are emitted as `NaN`/`+Inf`/`-Inf` per
//! Prometheus convention; non-numeric values are skipped.

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::Result;
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "prometheus";

#[derive(Debug, Clone)]
pub struct Config {
    /// Namespace prefix prepended to every metric. Default `glances`.
    pub prefix: String,
    /// Scrape-listen port for `GET /metrics`. Default 9091.
    pub port: u16,
    /// When set, also append the exposition to this file each tick.
    pub file: Option<String>,
    /// Include the unix-ms timestamp suffix on each sample. Default true.
    pub include_timestamp: bool,
    /// Optional override for the timestamp (seconds since epoch).
    pub timestamp: Option<f64>,
    /// Static labels on every sample. Upstream default `src:glances`
    /// (config `labels`, `key:value,...` form).
    pub labels: Vec<(String, String)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: "glances".into(),
            port: 9091,
            file: None,
            include_timestamp: true,
            timestamp: None,
            labels: vec![("src".into(), "glances".into())],
        }
    }
}

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

/// Latest rendered exposition, served by the scrape listener.
fn latest() -> &'static Mutex<String> {
    static L: OnceLock<Mutex<String>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(String::new()))
}

fn listener_started() -> &'static Mutex<Option<u16>> {
    static S: OnceLock<Mutex<Option<u16>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Spawn the `/metrics` scrape listener exactly once.
fn ensure_listener(port: u16) {
    let mut started = listener_started().lock().unwrap_or_else(|e| e.into_inner());
    if started.is_some() { return; }
    *started = Some(port);
    std::thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => l,
            Err(e) => {
                crate::core::logger::error(&format!(
                    "prometheus scrape listener bind :{} failed: {}", port, e
                ));
                return;
            }
        };
        for conn in listener.incoming() {
            if let Ok(mut s) = conn {
                let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let mut head = [0u8; 1024];
                use std::io::Read;
                let n = s.read(&mut head).unwrap_or(0);
                let req = String::from_utf8_lossy(&head[..n]);
                let ok = req.starts_with("GET /metrics ");
                let (status, body) = if ok {
                    ("200 OK", latest().lock().unwrap_or_else(|e| e.into_inner()).clone())
                } else {
                    ("404 Not Found", "try GET /metrics\n".to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status, body.len(), body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        }
    });
}

/// Render + publish the snapshot (listener, file sink).
pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let ts_ms = (cfg.timestamp.unwrap_or_else(now_secs) * 1000.0) as i64;
    let ts_str = if cfg.include_timestamp { format!(" {}", ts_ms) } else { String::new() };
    let mut buf = Vec::<u8>::new();
    render(fields, cfg, &mut buf, &ts_str)?;
    let text = String::from_utf8_lossy(&buf).into_owned();

    *latest().lock().unwrap_or_else(|e| e.into_inner()) = text.clone();
    ensure_listener(cfg.port);

    if let Some(path) = cfg.file.as_ref() {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        f.write_all(text.as_bytes())?;
    }
    Ok(())
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
fn help_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

/// Metric-name charset: `[a-zA-Z0-9_:]`; anything else → `_`.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == ':' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};

    fn obj(pairs: &[(&str, Value)]) -> Value {
        let mut m = BTreeMap::new();
        for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
        Value::Object(m)
    }

    fn flat(snap: &Value) -> Vec<Field<'_>> {
        crate::exports::flatten::collect(snap, &HashMap::new())
    }

    fn render_string(fields: &[Field<'_>], cfg: &Config, ts: &str) -> String {
        let mut buf = Vec::new();
        render(fields, cfg, &mut buf, ts).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn timestamp_is_milliseconds() {
        let snap = obj(&[("cpu", obj(&[("total", Value::Float(1.0))]))]);
        let cfg = Config::default();
        // 1700000000 s → 1700000000000 ms.
        let out = render_string(&flat(&snap), &cfg, " 1700000000000");
        assert!(out.contains("glances_cpu_total{src=\"glances\"} 1 1700000000000"), "got: {}", out);
    }

    #[test]
    fn samples_carry_src_and_element_labels() {
        // Upstream parity: default src label + per-element identity label.
        let nic = obj(&[
            ("interface_name", Value::String("eth0".into())),
            ("bytes_recv", Value::Uint(10)),
        ]);
        let snap = obj(&[("network", Value::Array(vec![nic]))]);
        let mut keys = HashMap::new();
        keys.insert("network".to_string(), "interface_name");
        let fields = crate::exports::flatten::collect(&snap, &keys);
        let cfg = Config::default();
        let out = render_string(&fields, &cfg, "");
        assert!(
            out.contains("glances_network_bytes_recv{src=\"glances\",interface_name=\"eth0\"} 10"),
            "got: {}", out
        );
    }

    #[test]
    fn parse_labels_reads_key_value_pairs() {
        assert_eq!(
            parse_labels("src:glances,host:web1"),
            vec![("src".to_string(), "glances".to_string()), ("host".to_string(), "web1".to_string())]
        );
        assert!(parse_labels("bogus").is_empty());
    }

    #[test]
    fn sanitize_whitelists_metric_charset() {
        assert_eq!(sanitize("cpu total"), "cpu_total");
        assert_eq!(sanitize("fs./"), "fs__");
        assert_eq!(sanitize("a%b(c)"), "a_b_c_");
        assert_eq!(sanitize("ok:name_1"), "ok:name_1");
    }

    #[test]
    fn help_escapes_backslash_and_newline() {
        let snap = obj(&[("a\\b", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config::default();
        let out = render_string(&flat(&snap), &cfg, "");
        assert!(out.contains("# HELP glances_a_b_x a\\\\b.x"), "got: {}", out);
    }

    #[test]
    fn non_numeric_values_skipped() {
        let snap = obj(&[("cpu", obj(&[
            ("n", Value::String("x".into())),
            ("v", Value::Float(2.0)),
        ]))]);
        let cfg = Config::default();
        let out = render_string(&flat(&snap), &cfg, "");
        assert!(!out.contains("glances_cpu_n"));
        assert!(out.contains("glances_cpu_v{src=\"glances\"} 2"));
    }
}
