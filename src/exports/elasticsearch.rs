//! Elasticsearch `_bulk` exporter — POST newline-delimited JSON actions
//! + documents to `POST /_bulk`. Each refresh tick emits one action
//! (`{"index":{"_index":"<idx>","_id":"<plugin>.<key>"}}`) followed by
//! the document, both terminated by `\n`. ES requires a trailing `\n`
//! after the last document.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "elasticsearch";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub index: String,
    pub auth_user: Option<String>,
    pub auth_pass: Option<String>,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9200,
            index: "glances".into(),
            auth_user: None,
            auth_pass: None,
            timeout_secs: 5,
        }
    }
}

/// Build the HTTP request bytes. Exposed for unit tests.
pub fn build_request(cfg: &Config, body: &str) -> Vec<u8> {
    let mut req = Vec::with_capacity(body.len() + 256);
    let host_header = format!("{}:{}", cfg.host, cfg.port);
    write!(
        req,
        "POST /_bulk HTTP/1.1\r\nHost: {}\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n",
        host_header, body.len(),
    ).unwrap();
    if let (Some(u), Some(p)) = (cfg.auth_user.as_ref(), cfg.auth_pass.as_ref()) {
        let raw = format!("{}:{}", u, p);
        let encoded = base64_encode(raw.as_bytes());
        write!(req, "Authorization: Basic {}\r\n", encoded).unwrap();
    }
    req.extend_from_slice(b"\r\n");
    req.extend_from_slice(body.as_bytes());
    req
}

fn base64_encode(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

/// Quote a JSON string key. Mirrors the helper in `couchdb.rs`.
fn quote_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Days → civil (y, m, d) per Howard Hinnant's algorithm (std-only UTC).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Unix seconds → `(date "YYYY.MM.DD", ISO-8601 UTC)` pair.
pub fn date_and_iso(secs: i64) -> (String, String) {
    let days = secs.div_euclid(86400);
    let tod = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    (
        format!("{:04}.{:02}.{:02}", y, m, d),
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, m, d, hh, mm, ss),
    )
}

/// Flatten one plugin's value into string columns (upstream
/// `build_export` parity: every value stringified via `str()`).
fn columns(value: &Value, prefix: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let name = if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) };
                columns(v, &name, out);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                let name = if prefix.is_empty() {
                    i.to_string()
                } else {
                    format!("{}.{}", prefix, i)
                };
                columns(v, &name, out);
            }
        }
        v => out.push((prefix.to_string(), json_stringify(v))),
    }
}

/// Scalar → string form (`str(value)` parity; JSON for containers).
fn json_stringify(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) if f.is_nan() || f.is_infinite() => "null".into(),
        Value::Float(f) => format!("{}", f),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => crate::core::value::to_json(v),
    }
}

/// One `index` action + doc per plugin (upstream parity): index
/// `<index>-YYYY.MM.DD`, `_id = <plugin>.<iso-ts>`, doc carries the
/// plugin's full column set plus `plugin`/`timestamp`.
pub(crate) fn build_body(snap: &Value, index: &str, date: &str, iso: &str) -> String {
    let plugins = match snap.as_object() {
        Some(o) => o,
        None => return String::new(),
    };
    let mut out = String::new();
    for (plugin, value) in plugins {
        let mut cols = Vec::new();
        columns(value, "", &mut cols);
        let mut doc = format!(
            "{{\"plugin\":{},\"timestamp\":{}",
            quote_str(plugin), quote_str(iso),
        );
        for (k, v) in &cols {
            doc.push_str(&format!(",{}:{}", quote_str(k), quote_str(v)));
        }
        doc.push_str("}\n");
        out.push_str(&format!(
            "{{\"index\":{{\"_index\":{},\"_id\":{},\"_type\":{}}}}}\n",
            quote_str(&format!("{}-{}", index, date)),
            quote_str(&format!("{}.{}", plugin, iso)),
            quote_str(&format!("glances-{}", plugin)),
        ));
        out.push_str(&doc);
    }
    out
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.index.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "elasticsearch exporter requires index".into(),
        ));
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64).unwrap_or(0);
    let (date, iso) = date_and_iso(secs);
    let body = build_body(snap, &cfg.index, &date, &iso);
    if body.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    let mut s = stream;
    let req = build_request(cfg, &body);
    s.write_all(&req)?;
    s.flush()?;
    Ok(())
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
    fn request_targets_bulk_endpoint() {
        let cfg = Config::default();
        let req = build_request(&cfg, "");
        let raw = String::from_utf8(req).unwrap();
        assert!(raw.starts_with("POST /_bulk HTTP/1.1\r\n"));
        assert!(raw.contains("Content-Type: application/x-ndjson"));
    }

    #[test]
    fn body_is_one_doc_per_plugin_with_dated_index() {
        // Upstream parity: index <name>-YYYY.MM.DD, _id <plugin>.<iso>,
        // doc carries the plugin's full column set as strings.
        let snap = obj(&[("cpu", obj(&[("total", Value::Int(42))]))]);
        let body = build_body(&snap, "glances", "2024.01.02", "2024-01-02T03:04:05");
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"_index\":\"glances-2024.01.02\""), "got: {}", lines[0]);
        assert!(lines[0].contains("\"_id\":\"cpu.2024-01-02T03:04:05\""), "got: {}", lines[0]);
        assert!(lines[0].contains("\"_type\":\"glances-cpu\""), "got: {}", lines[0]);
        assert!(lines[1].contains("\"plugin\":\"cpu\""), "got: {}", lines[1]);
        assert!(lines[1].contains("\"timestamp\":\"2024-01-02T03:04:05\""), "got: {}", lines[1]);
        assert!(lines[1].contains("\"total\":\"42\""), "got: {}", lines[1]);
        assert!(body.ends_with("}\n"));
    }

    #[test]
    fn date_and_iso_epoch_is_sane() {
        assert_eq!(
            date_and_iso(0),
            ("1970.01.01".to_string(), "1970-01-01T00:00:00".to_string())
        );
        // 2024-01-02T03:04:05Z = 1704164645.
        assert_eq!(
            date_and_iso(1704164645),
            ("2024.01.02".to_string(), "2024-01-02T03:04:05".to_string())
        );
    }

    #[test]
    fn nan_values_render_as_null_in_doc() {
        let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
        let body = build_body(&snap, "glances", "2024.01.02", "2024-01-02T03:04:05");
        assert!(body.contains("\"bad\":\"null\""), "got: {}", body);
    }

    #[test]
    fn auth_header_emitted_when_credentials_set() {
        let cfg = Config {
            auth_user: Some("u".into()),
            auth_pass: Some("p".into()),
            ..Default::default()
        };
        let req = build_request(&cfg, "");
        let raw = String::from_utf8(req).unwrap();
        // base64("u:p") = "dTpw"
        assert!(raw.contains("Authorization: Basic dTpw"));
    }

    #[test]
    fn empty_index_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { index: String::new(), ..Default::default() };
        let err = write(&snap, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}