//! Cassandra CQL exporter — real native-protocol v4 client. Performs
//! the `STARTUP → READY` handshake, then ships all per-tick INSERTs as
//! one unlogged `BATCH` request (one round-trip per refresh).
//!
//! Auth-capable clusters are out of scope: an `AUTHENTICATE` response
//! aborts the write with an error rather than pretending success.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "cassandra";

const PROTO_VERSION_REQ: u8 = 0x04;
const PROTO_VERSION_RESP: u8 = 0x84;
const OP_ERROR: u8 = 0x00;
const OP_STARTUP: u8 = 0x01;
const OP_READY: u8 = 0x02;
const OP_AUTHENTICATE: u8 = 0x03;
const OP_RESULT: u8 = 0x08;
const OP_BATCH: u8 = 0x0D;
const BATCH_UNLOGGED: u8 = 0;
const CONSISTENCY_ONE: i16 = 0x0001;
/// Response payload cap — a sane server never sends more in a READY/
/// RESULT/ERROR frame for our requests.
const MAX_FRAME_BODY: usize = 1 << 20;

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

/// One v4 request frame: `ver|flags|stream|opcode|len|body`.
fn frame(opcode: u8, stream: i16, body: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(9 + body.len());
    f.push(PROTO_VERSION_REQ);
    f.push(0); // flags: no compression/tracing
    f.extend_from_slice(&stream.to_be_bytes());
    f.push(opcode);
    f.extend_from_slice(&(body.len() as i32).to_be_bytes());
    f.extend_from_slice(body);
    f
}

/// CQL `[string]` = u16 len + bytes.
fn put_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u16).to_be_bytes());
    buf.extend_from_slice(s.as_bytes());
}
/// CQL `[long string]` = i32 len + bytes.
fn put_long_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as i32).to_be_bytes());
    buf.extend_from_slice(s.as_bytes());
}
/// CQL `[string map]` = u16 count + `[string]` pairs.
fn put_string_map(buf: &mut Vec<u8>, pairs: &[(&str, &str)]) {
    buf.extend_from_slice(&(pairs.len() as u16).to_be_bytes());
    for (k, v) in pairs {
        put_string(buf, k);
        put_string(buf, v);
    }
}

/// STARTUP body: `{CQL_VERSION: "3.0.0"}`.
pub fn build_startup() -> Vec<u8> {
    let mut body = Vec::new();
    put_string_map(&mut body, &[("CQL_VERSION", "3.0.0")]);
    frame(OP_STARTUP, 0, &body)
}

/// BATCH body: type + [query kind=0][long-string cql][u16 n_values=0]…
/// + consistency + flags(0).
pub fn build_batch(statements: &[String]) -> Vec<u8> {
    let mut body = Vec::new();
    body.push(BATCH_UNLOGGED);
    body.extend_from_slice(&(statements.len() as u16).to_be_bytes());
    for q in statements {
        body.push(0); // kind: plain query string
        put_long_string(&mut body, q);
        body.extend_from_slice(&0u16.to_be_bytes()); // no bound values
    }
    body.extend_from_slice(&CONSISTENCY_ONE.to_be_bytes());
    body.push(0); // flags: no serial/timestamp/names
    frame(OP_BATCH, 1, &body)
}

/// Read one response frame → `(opcode, body)`. Errors on bad version,
/// oversized body, or short read.
fn read_frame(s: &mut TcpStream) -> Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 9];
    s.read_exact(&mut head)?;
    if head[0] != PROTO_VERSION_RESP {
        return Err(GlancesError::Parse(format!(
            "cassandra: bad response version {:#x}", head[0],
        )));
    }
    let opcode = head[4];
    let len = i32::from_be_bytes([head[5], head[6], head[7], head[8]]) as usize;
    if len > MAX_FRAME_BODY {
        return Err(GlancesError::Parse(format!(
            "cassandra: frame body too large ({} bytes)", len,
        )));
    }
    let mut body = vec![0u8; len];
    s.read_exact(&mut body)?;
    Ok((opcode, body))
}

/// Extract the error message from an ERROR frame body for reporting.
fn error_message(body: &[u8]) -> String {
    // ERROR body: i32 code + [string] message.
    if body.len() < 6 { return format!("code-only ({}B)", body.len()); }
    let mlen = u16::from_be_bytes([body[4], body[5]]) as usize;
    if body.len() < 6 + mlen { return "truncated message".into(); }
    String::from_utf8_lossy(&body[6..6 + mlen]).into_owned()
}

/// Render a single CQL INSERT for one field. `series` keeps array-plugin
/// elements distinct (`network.eth0`).
pub fn render_insert(cfg: &Config, series: &str, key: &str, value: &str) -> String {
    format!(
        "INSERT INTO {}.{} (plugin, key, value) VALUES ('{}', '{}', '{}');",
        cfg.keyspace, cfg.table,
        escape_cql(series), escape_cql(key), escape_cql(value),
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

pub(crate) fn build_statements(fields: &[Field<'_>], cfg: &Config) -> Vec<String> {
    fields.iter()
        .map(|f| render_insert(cfg, &f.series, f.key, &v_to_str(f.value)))
        .collect()
}

/// Identifiers land inside CQL — restrict to `[A-Za-z0-9_]`.
fn valid_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    if !valid_ident(&cfg.keyspace) || !valid_ident(&cfg.table) {
        return Err(GlancesError::InvalidConfig(
            "cassandra keyspace/table must match [A-Za-z0-9_]+".into(),
        ));
    }
    let statements = build_statements(fields, cfg);
    if statements.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_write_timeout(Some(timeout))?;
    s.set_read_timeout(Some(timeout))?;

    // Handshake: STARTUP → expect READY (AUTHENTICATE = unsupported).
    s.write_all(&build_startup())?;
    match read_frame(&mut s)? {
        (OP_READY, _) => {}
        (OP_AUTHENTICATE, _) => {
            return Err(GlancesError::Other(
                "cassandra: server requires authentication (unsupported)".into(),
            ));
        }
        (OP_ERROR, body) => {
            return Err(GlancesError::Other(format!(
                "cassandra STARTUP rejected: {}", error_message(&body),
            )));
        }
        (op, _) => {
            return Err(GlancesError::Other(format!(
                "cassandra: unexpected opcode {:#x} during handshake", op,
            )));
        }
    }

    // Batch the tick's INSERTs; expect RESULT (any kind) or ERROR.
    s.write_all(&build_batch(&statements))?;
    match read_frame(&mut s)? {
        (OP_RESULT, _) => Ok(()),
        (OP_ERROR, body) => Err(GlancesError::Other(format!(
            "cassandra BATCH failed: {}", error_message(&body),
        ))),
        (op, _) => Err(GlancesError::Other(format!(
            "cassandra: unexpected opcode {:#x} after BATCH", op,
        ))),
    }
}
