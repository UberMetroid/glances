//! Cassandra CQL exporter — builds CQL `INSERT` statements and ships them
//! over a TCP socket to a Cassandra-compatible node (default port 9042).
//!
//! PARTIAL: real CQL binary protocol (v3/v4) framing is out of scope; this
//! exporter emits ready-to-execute CQL text and ships it as one TCP write
//! per refresh tick. A companion `cqlsh` script or `cassandra-stress`
//! setup that listens on the configured port can consume the bytes.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "cassandra";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub keyspace: String,
    pub table: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9042,
            keyspace: "glances".into(),
            table: "stats".into(),
            timeout_secs: 5,
        }
    }
}

/// Render a single CQL INSERT for one (plugin, key, value) tuple.
pub fn render_insert(cfg: &Config, plugin: &str, key: &str, value: &str) -> String {
    format!(
        "INSERT INTO {}.{} (plugin, key, value) VALUES ('{}', '{}', '{}');",
        cfg.keyspace,
        cfg.table,
        escape_cql(plugin),
        escape_cql(key),
        escape_cql(value),
    )
}

fn escape_cql(s: &str) -> String {
    s.replace('\'', "''")
}

fn v_to_str(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) if f.is_nan() || f.is_infinite() => "NULL".into(),
        Value::Float(f) => format!("{}", f),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => s.clone(),
        Value::Null | Value::Array(_) | Value::Object(_) => "NULL".into(),
    }
}

pub(crate) fn build_body(snap: &Value, cfg: &Config) -> String {
    let plugins = match snap.as_object() { Some(o) => o, None => return String::new() };
    let mut out = String::new();
    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        for (k, v) in fields {
            out.push_str(&render_insert(cfg, plugin, k, &v_to_str(v)));
            out.push('\n');
        }
    }
    out
}

/// Open one TCP connection and ship the CQL statements; if the connect or
/// write fails, retry once after a 5s backoff before returning Err.
pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.keyspace.is_empty() || cfg.table.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "cassandra exporter requires keyspace + table".into(),
        ));
    }
    let body = build_body(snap, cfg);
    if body.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter
        .next()
        .ok_or_else(|| GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host)))?;

    try_send(&addr, cfg.timeout_secs, &body).or_else(|first_err| {
        eprintln!("[{}] first send failed: {}; retrying in 5s", NAME, first_err);
        std::thread::sleep(Duration::from_secs(5));
        try_send(&addr, cfg.timeout_secs, &body).map_err(|e| GlancesError::Other(format!(
            "{} export failed after retry: {} (first: {})", NAME, e, first_err)))
    })
}

fn try_send(addr: &std::net::SocketAddr, timeout_secs: u64, body: &str) -> Result<()> {
    let timeout = Duration::from_secs(timeout_secs);
    let stream = TcpStream::connect_timeout(addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    let mut s = stream;
    s.write_all(body.as_bytes())?;
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
    fn render_insert_uses_keyspace_and_table() {
        let cfg = Config::default();
        let sql = render_insert(&cfg, "cpu", "total", "42");
        assert_eq!(
            sql,
            "INSERT INTO glances.stats (plugin, key, value) VALUES ('cpu', 'total', '42');"
        );
    }

    #[test]
    fn escapes_apostrophes_by_doubling() {
        let cfg = Config::default();
        let sql = render_insert(&cfg, "cpu", "name", "it's ok");
        assert!(sql.contains("'it''s ok'"));
    }

    #[test]
    fn build_body_emits_one_insert_per_field() {
        let cfg = Config::default();
        let snap = obj(&[("cpu", obj(&[
            ("total", Value::Int(42)),
            ("user", Value::Float(3.5)),
        ]))]);
        let body = build_body(&snap, &cfg);
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("VALUES ('cpu', 'total', '42')"));
        assert!(lines[1].contains("VALUES ('cpu', 'user', '3.5')"));
    }

    #[test]
    fn nan_renders_as_null_token() {
        let cfg = Config::default();
        let sql = render_insert(&cfg, "cpu", "bad", &v_to_str(&Value::Float(f64::NAN)));
        assert!(sql.ends_with("'NULL');"));
    }

    #[test]
    fn empty_keyspace_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { keyspace: String::new(), ..Default::default() };
        let err = write(&snap, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}