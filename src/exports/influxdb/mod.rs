//! InfluxDB v1.x line-protocol exporter over HTTP.
//!
//! Per refresh tick, `POST /write?db=<database>` with the line-protocol
//! body to `host:port` (8086 is InfluxDB's HTTP endpoint — there is no
//! raw-TCP LP listener). When `file` is set, the body is appended to
//! that path instead.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::exports::flatten::Field;

mod format;

pub(crate) use format::format_lines;
pub use format::parse_tags;

pub const NAME: &str = "influxdb";

pub struct Config {
    pub host: String,
    pub port: u16,
    /// Database name for `?db=`. Default `glances`.
    pub database: String,
    /// Optional measurement prefix (`<prefix>.<plugin>`). Default none.
    pub prefix: String,
    /// Extra `key:value,...` tags on every line. Default none.
    pub tags: Vec<(String, String)>,
    /// Hostname tag (upstream always tags `hostname`). Default none.
    pub hostname: String,
    pub user: Option<String>,
    pub password: Option<String>,
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
            prefix: String::new(),
            tags: Vec::new(),
            hostname: String::new(),
            user: None,
            password: None,
            file: None,
            timestamp: None,
            timeout_secs: 5,
        }
    }
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
        "POST /write?db={} HTTP/1.1\r\nHost: {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n",
        cfg.database, host_header, body.len(),
    ).unwrap();
    if let Some(u) = cfg.user.as_ref() {
        let raw = format!("{}:{}", u, cfg.password.as_deref().unwrap_or(""));
        write!(req, "Authorization: Basic {}\r\n", base64_encode(raw.as_bytes())).unwrap();
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

/// Send the snapshot as line-protocol: HTTP POST, or file append when
/// `cfg.file` is set.
pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let body = format_lines(fields, cfg, cfg.timestamp);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::value::Value;
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

    fn cfg() -> Config {
        Config {
            hostname: "testhost".into(),
            ..Default::default()
        }
    }

    #[test]
    fn lines_skip_empty_field_sets() {
        // A series whose only field is NaN must not emit `series  <ts>`.
        let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
        let body = format_lines(&flat(&snap), &cfg(), Some(1.0));
        assert!(body.is_empty(), "empty field set produced: {:?}", body);
    }

    #[test]
    fn lines_group_same_plugin_fields_with_second_precision() {
        // Upstream parity: plugin measurement, hostname tag, numbers as
        // floats, second timestamps (time_precision="s").
        let snap = obj(&[("cpu", obj(&[
            ("total", Value::Float(1.5)),
            ("user", Value::Int(2)),
        ]))]);
        let body = format_lines(&flat(&snap), &cfg(), Some(1.0));
        assert!(body.starts_with("cpu,hostname=testhost total=1.5,user=2.0 1\n"), "got: {}", body);
    }

    #[test]
    fn array_plugin_measurement_is_plugin_with_key_tag() {
        // Upstream parity: element identity is a tag, not the name.
        let nic = obj(&[
            ("interface_name", Value::String("eth0".into())),
            ("rx", Value::Uint(10)),
        ]);
        let snap = obj(&[("network", Value::Array(vec![nic]))]);
        let mut keys = HashMap::new();
        keys.insert("network".to_string(), "interface_name");
        let body = format_lines(&crate::exports::flatten::collect(&snap, &keys), &cfg(), Some(1.0));
        assert!(
            body.starts_with("network,hostname=testhost,interface_name=eth0 rx=10.0 1\n"),
            "got: {}", body
        );
    }

    #[test]
    fn name_fields_promote_to_tags() {
        // Upstream FIELD_TO_TAG parity: name/cmdline/type are tags.
        let proc = obj(&[
            ("name", Value::String("py".into())),
            ("cpu_percent", Value::Float(3.0)),
        ]);
        let snap = obj(&[("processlist", Value::Array(vec![proc]))]);
        let mut keys = HashMap::new();
        keys.insert("processlist".to_string(), "pid");
        let body = format_lines(&crate::exports::flatten::collect(&snap, &keys), &cfg(), Some(1.0));
        assert!(body.contains("name=py"), "got: {}", body);
        assert!(!body.contains("name=\"py\""), "name must be a tag, got: {}", body);
    }

    #[test]
    fn prefix_prepends_measurement() {
        let snap = obj(&[("cpu", obj(&[("total", Value::Float(1.0))]))]);
        let c = Config { prefix: "p".into(), ..cfg() };
        let body = format_lines(&flat(&snap), &c, Some(1.0));
        assert!(body.starts_with("p.cpu,"), "got: {}", body);
    }

    #[test]
    fn huge_uint_does_not_wrap_negative() {
        let snap = obj(&[("x", obj(&[("big", Value::Uint(u64::MAX))]))]);
        let body = format_lines(&flat(&snap), &cfg(), Some(1.0));
        assert!(body.contains("big=18446744073709551615.0"), "got: {}", body);
        assert!(!body.contains("-1i"));
    }

    #[test]
    fn auth_header_emitted_when_user_set() {
        let c = Config { user: Some("u".into()), password: Some("p".into()), ..Default::default() };
        let raw = String::from_utf8(build_request(&c, "x")).unwrap();
        // base64("u:p") = "dTpw"
        assert!(raw.contains("Authorization: Basic dTpw"), "got: {}", raw);
    }

    #[test]
    fn empty_database_without_file_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { database: String::new(), ..Default::default() };
        let err = write(&flat(&snap), &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}
