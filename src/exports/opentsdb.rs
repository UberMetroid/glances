//! OpenTSDB `telnet put` exporter — connects to a TSDB node and emits
//! one `put <metric> <timestamp> <value> [<tagk>=<tagv> ...]\n` line per
//! field. Metric/tag names are whitelisted to the OpenTSDB charset
//! `[a-zA-Z0-9_./-]`; NaN / Infinity are skipped.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "opentsdb";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 4242,
            timeout_secs: 5,
        }
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Build one `put` line for a (metric, value) pair. `metric` is the
/// series (`plugin` or `plugin.elem`), `key` the field name; tags carry
/// `plugin=<plugin>` and `elem=<elem>` when present.
pub fn render_put(f: &Field<'_>, value: f64, ts: i64) -> Option<String> {
    if value.is_nan() || value.is_infinite() { return None; }
    let metric = format!("{}.{}", sanitize(&f.series), sanitize(f.key));
    let mut tags = format!(" plugin={}", sanitize(f.plugin));
    if let Some(e) = f.elem.as_ref() {
        tags.push_str(&format!(" elem={}", sanitize(e)));
    }
    Some(format!("put {} {} {}{}\n", metric, ts, value, tags))
}

/// OpenTSDB allows `[a-zA-Z0-9_./-]` in metric and tag names; anything
/// else (spaces, `=`, quotes, unicode) becomes `_`.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-') { c } else { '_' })
        .collect()
}

pub fn build_body(fields: &[Field<'_>], ts: i64) -> String {
    let mut out = String::new();
    for f in fields {
        let n = match f.value {
            Value::Int(i) => Some(*i as f64),
            Value::Uint(u) => Some(*u as f64),
            Value::Float(v) if v.is_nan() || v.is_infinite() => None,
            Value::Float(v) => Some(*v),
            _ => None,
        };
        if let Some(n) = n {
            if let Some(line) = render_put(f, n, ts) {
                out.push_str(&line);
            }
        }
    }
    out
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let body = build_body(fields, now_secs());
    if body.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_write_timeout(Some(timeout))?;
    s.write_all(body.as_bytes())?;
    s.flush()?;
    Ok(())
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

    #[test]
    fn render_put_produces_put_command() {
        let snap = obj(&[("cpu", obj(&[("total", Value::Float(42.0))]))]);
        let f = flat(&snap);
        let line = render_put(&f[0], 42.0, 1_700_000_000).unwrap();
        assert_eq!(line, "put cpu.total 1700000000 42 plugin=cpu\n");
    }

    #[test]
    fn sanitize_whitelists_opentsdb_charset() {
        assert_eq!(sanitize("cpu 0"), "cpu_0");
        assert_eq!(sanitize("a=b'c"), "a_b_c");
        assert_eq!(sanitize("ok-1.2/3_x"), "ok-1.2/3_x");
    }

    #[test]
    fn render_put_skips_nan_and_inf() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Float(1.0))]))]);
        let f = flat(&snap);
        assert!(render_put(&f[0], f64::NAN, 0).is_none());
        assert!(render_put(&f[0], f64::INFINITY, 0).is_none());
    }

    #[test]
    fn build_body_emits_one_put_per_numeric_field() {
        let snap = obj(&[(
            "cpu",
            obj(&[
                ("good", Value::Int(1)),
                ("bad", Value::Float(f64::NAN)),
                ("str", Value::String("hi".into())),
            ]),
        )]);
        let body = build_body(&flat(&snap), 100);
        assert!(body.contains("put cpu.good 100 1 plugin=cpu\n"));
        assert!(!body.contains("bad"));
        assert!(!body.contains("str"));
    }

    #[test]
    fn array_elements_get_elem_tag() {
        let mut nic = BTreeMap::new();
        nic.insert("iface".into(), Value::String("eth0".into()));
        nic.insert("rx".into(), Value::Uint(5));
        let snap = obj(&[("network", Value::Array(vec![Value::Object(nic)]))]);
        let mut keys = HashMap::new();
        keys.insert("network".to_string(), "iface");
        let body = build_body(&crate::exports::flatten::collect(&snap, &keys), 100);
        assert!(body.contains("put network.eth0.rx 100 5 plugin=network elem=eth0\n"), "got: {}", body);
    }

    #[test]
    fn empty_snapshot_yields_empty_body() {
        let snap = Value::Object(BTreeMap::new());
        assert!(build_body(&flat(&snap), 100).is_empty());
    }
}
