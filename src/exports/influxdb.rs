//! InfluxDB v1.x line-protocol exporter over HTTP.
//!
//! Per refresh tick, `POST /write?db=<database>` with the line-protocol
//! body to `host:port` (8086 is InfluxDB's HTTP endpoint — there is no
//! raw-TCP LP listener). When `file` is set, the body is appended to
//! that path instead.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "influxdb";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// Database name for `?db=`. Default `glances`.
    pub database: String,
    /// When set, append the LP body to this file instead of POSTing.
    pub file: Option<String>,
    /// Optional override for the timestamp (seconds since epoch).
    pub timestamp: Option<f64>,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8086,
            database: "glances".into(),
            file: None,
            timestamp: None,
            timeout_secs: 5,
        }
    }
}

fn now_nanos() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Build the `POST /write?db=…` request bytes. Exposed for unit tests.
pub fn build_request(cfg: &Config, body: &str) -> Vec<u8> {
    let mut req = Vec::with_capacity(body.len() + 256);
    let host_header = if cfg.host.contains(':') {
        format!("[{}]:{}", cfg.host, cfg.port)
    } else {
        format!("{}:{}", cfg.host, cfg.port)
    };
    write!(
        req,
        "POST /write?db={} HTTP/1.1\r\nHost: {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        cfg.database, host_header, body.len(),
    ).unwrap();
    req.extend_from_slice(body.as_bytes());
    req
}

/// Send the snapshot as line-protocol: HTTP POST, or file append when
/// `cfg.file` is set.
pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let body = format_lines(fields, cfg.timestamp);
    if body.is_empty() { return Ok(()); }
    if let Some(path) = cfg.file.as_ref() {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        f.write_all(body.as_bytes())?;
        return Ok(());
    }
    if cfg.database.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "influxdb exporter requires a database (or --export-influxdb-file)".into(),
        ));
    }
    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;
    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_write_timeout(Some(timeout))?;
    s.write_all(&build_request(cfg, &body))?;
    s.flush()?;
    Ok(())
}

/// Group flat fields into `measurement field=v,… ts` lines. Series are
/// already element-qualified (`network.eth0`) by the flatten pass.
fn format_lines(fields: &[Field<'_>], ts_override: Option<f64>) -> String {
    let ts = match ts_override {
        Some(s) => (s * 1e9) as i64,
        None => now_nanos(),
    };
    // Group consecutive same-series fields into one LP line.
    let mut out = String::new();
    let mut cur_series = String::new();
    let mut cur_fields: Vec<String> = Vec::new();
    let flush = |series: &str, fs: &mut Vec<String>, out: &mut String| {
        if fs.is_empty() { return; } // no fields → invalid LP line; skip
        out.push_str(&escape_measurement(series));
        out.push(' ');
        out.push_str(&fs.join(","));
        out.push(' ');
        out.push_str(&ts.to_string());
        out.push('\n');
        fs.clear();
    };
    for f in fields {
        if f.series != cur_series {
            flush(&cur_series, &mut cur_fields, &mut out);
            cur_series = f.series.clone();
        }
        if let Some(s) = field_to_lp(f.key, f.value) {
            cur_fields.push(s);
        }
    }
    flush(&cur_series, &mut cur_fields, &mut out);
    out
}

fn field_to_lp(key: &str, v: &Value) -> Option<String> {
    let k = escape_tag(key);
    let rendered = match v {
        Value::Int(i) => i.to_string() + "i",
        // v1 LP has no unsigned suffix — clamp to i64::MAX rather than
        // wrap negative server-side.
        Value::Uint(u) if *u > i64::MAX as u64 => format!("{}", u),
        Value::Uint(u) => u.to_string() + "i",
        Value::Float(f) if f.is_nan() || f.is_infinite() => return None,
        Value::Float(f) => format!("{}", f),
        Value::Bool(b) => b.to_string(),
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
    fn request_posts_to_write_endpoint() {
        let cfg = Config::default();
        let req = build_request(&cfg, "x");
        let raw = String::from_utf8(req).unwrap();
        assert!(raw.starts_with("POST /write?db=glances HTTP/1.1\r\n"));
        assert!(raw.contains("Content-Type: text/plain"));
    }

    #[test]
    fn lines_skip_empty_field_sets() {
        // A series whose only field is NaN must not emit `series  <ts>`.
        let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
        let body = format_lines(&flat(&snap), Some(1.0));
        assert!(body.is_empty(), "empty field set produced: {:?}", body);
    }

    #[test]
    fn lines_group_same_series_fields() {
        let snap = obj(&[("cpu", obj(&[
            ("total", Value::Float(1.5)),
            ("user", Value::Int(2)),
        ]))]);
        let body = format_lines(&flat(&snap), Some(1.0));
        assert!(body.starts_with("cpu total=1.5,user=2i "), "got: {}", body);
        assert!(body.ends_with(" 1000000000\n"));
    }

    #[test]
    fn array_plugin_series_flatten() {
        let nic = obj(&[
            ("interface_name", Value::String("eth0".into())),
            ("rx", Value::Uint(10)),
        ]);
        let snap = obj(&[("network", Value::Array(vec![nic]))]);
        let mut keys = HashMap::new();
        keys.insert("network".to_string(), "interface_name");
        let body = format_lines(&crate::exports::flatten::collect(&snap, &keys), Some(1.0));
        assert!(body.starts_with("network.eth0 rx=10i "), "got: {}", body);
    }

    #[test]
    fn huge_uint_does_not_wrap_negative() {
        let snap = obj(&[("x", obj(&[("big", Value::Uint(u64::MAX))]))]);
        let body = format_lines(&flat(&snap), Some(1.0));
        assert!(body.contains("big=18446744073709551615"), "got: {}", body);
        assert!(!body.contains("-1i"));
    }

    #[test]
    fn empty_database_without_file_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { database: String::new(), ..Default::default() };
        let err = write(&flat(&snap), &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}
