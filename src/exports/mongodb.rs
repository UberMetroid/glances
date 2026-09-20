//! MongoDB OP_MSG exporter — binary `OP_MSG` request over TCP carrying a
//! real `insert` command: `{insert: <coll>, documents: [<doc>], $db: <db>}`.
//!
//! One document per tick holds the flattened fields
//! (`{series, plugin, key, value, ts}` per field is too chatty, so the
//! doc is `{series: {key: value}}` grouped per series — same shape as
//! Python Glances' single-document insert).
//!
//! PARTIAL: a real mongod also expects a `hello` handshake and returns
//! an OP_MSG reply we don't parse; the insert frame itself is fully
//! formed and byte-exact.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "mongodb";

const OP_MSG_OPCODE: i32 = 2013;
const FLAG_NONE: i32 = 0;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub collection: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 27017,
            database: "glances".into(),
            collection: "stats".into(),
            timeout_secs: 5,
        }
    }
}

/// BSON element type tags (subset we need).
const BSON_DOUBLE: u8 = 0x01;
const BSON_STRING: u8 = 0x02;
const BSON_EMB_DOC: u8 = 0x03;
const BSON_ARRAY: u8 = 0x04;
const BSON_BOOL: u8 = 0x08;
const BSON_NULL: u8 = 0x0A;
const BSON_INT64: u8 = 0x12;

/// Append a BSON element: `type || cstring(key) || value`.
fn put_bson_element(buf: &mut Vec<u8>, key: &str, v: &Value) {
    let (tag, payload): (u8, Vec<u8>) = match v {
        // Python ints are 64-bit — always emit int64, never a truncating
        // int32 (a >2GiB counter would wrap negative).
        Value::Int(i) => (BSON_INT64, i.to_le_bytes().to_vec()),
        Value::Uint(u) => (BSON_INT64, (*u as i64).to_le_bytes().to_vec()),
        Value::Float(f) if f.is_nan() || f.is_infinite() => (BSON_NULL, Vec::new()),
        Value::Float(f) => (BSON_DOUBLE, f.to_le_bytes().to_vec()),
        Value::Bool(b) => (BSON_BOOL, vec![u8::from(*b)]),
        Value::String(s) => {
            let bytes = s.as_bytes();
            let mut p = Vec::with_capacity(bytes.len() + 5);
            p.extend_from_slice(&((bytes.len() as i32) + 1).to_le_bytes());
            p.extend_from_slice(bytes);
            p.push(0);
            (BSON_STRING, p)
        }
        Value::Array(arr) => {
            // BSON array = embedded doc with keys "0","1",…
            let elems: Vec<(String, Value)> = arr.iter().enumerate()
                .map(|(i, x)| (i.to_string(), x.clone())).collect();
            (BSON_ARRAY, bson_document_pairs(&elems))
        }
        Value::Object(o) => {
            let elems: Vec<(String, Value)> = o.iter()
                .map(|(k, x)| (k.clone(), x.clone())).collect();
            (BSON_EMB_DOC, bson_document_pairs(&elems))
        }
        Value::Null => (BSON_NULL, Vec::new()),
    };
    buf.push(tag);
    buf.extend_from_slice(key.as_bytes());
    buf.push(0);
    buf.extend_from_slice(&payload);
}

/// Serialize a BSON document from owned key/value pairs.
fn bson_document_pairs(pairs: &[(String, Value)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (k, v) in pairs {
        put_bson_element(&mut body, k, v);
    }
    body.push(0);
    let mut doc = Vec::with_capacity(body.len() + 4);
    doc.extend_from_slice(&((body.len() as i32) + 4).to_le_bytes());
    doc.extend(body);
    doc
}

/// Serialize a BSON document from an ordered key/value slice (tests).
pub fn bson_document(pairs: &[(&str, Value)]) -> Vec<u8> {
    let owned: Vec<(String, Value)> = pairs.iter()
        .map(|(k, v)| (k.to_string(), v.clone())).collect();
    bson_document_pairs(&owned)
}

/// Build the OP_MSG request frame (message header + flag bits + body BSON).
pub fn build_op_msg(bson_body: &[u8]) -> Vec<u8> {
    let mut body_section = Vec::with_capacity(bson_body.len() + 1);
    body_section.push(0u8); // section kind 0 = body
    body_section.extend_from_slice(bson_body);

    let mut msg = Vec::with_capacity(16 + 4 + body_section.len());
    let msg_length = (16 + 4 + body_section.len()) as i32;
    msg.extend_from_slice(&msg_length.to_le_bytes()); // messageLength
    msg.extend_from_slice(&1i32.to_le_bytes());       // requestID
    msg.extend_from_slice(&0i32.to_le_bytes());       // responseTo
    msg.extend_from_slice(&OP_MSG_OPCODE.to_le_bytes()); // opCode
    msg.extend_from_slice(&FLAG_NONE.to_le_bytes());
    msg.extend_from_slice(&body_section);
    msg
}

/// Build the insert command document:
/// `{insert: <coll>, documents: [{<series>: {key: value}, ...}], $db: <db>}`.
pub(crate) fn build_body(fields: &[Field<'_>], cfg: &Config) -> Vec<u8> {
    if fields.is_empty() { return Vec::new(); }
    // Group fields into per-series sub-documents (deterministic order —
    // series strings arrive grouped already from flatten::collect).
    let mut doc_pairs: Vec<(String, Value)> = Vec::new();
    let mut cur_series: Option<String> = None;
    let mut cur_fields: Vec<(String, Value)> = Vec::new();
    for f in fields {
        if cur_series.as_deref() != Some(f.series.as_str()) {
            if let Some(s) = cur_series.take() {
                let mut m = std::collections::BTreeMap::new();
                for (k, v) in cur_fields.drain(..) { m.insert(k, v); }
                doc_pairs.push((s, Value::Object(m)));
            }
            cur_series = Some(f.series.clone());
        }
        cur_fields.push((f.key.to_string(), f.value.clone()));
    }
    if let Some(s) = cur_series {
        let mut m = std::collections::BTreeMap::new();
        for (k, v) in cur_fields { m.insert(k, v); }
        doc_pairs.push((s, Value::Object(m)));
    }

    // Command doc: {insert: <coll>, documents: [stats_doc], $db: <db>}.
    let mut body = Vec::new();
    put_bson_element(&mut body, "insert", &Value::String(cfg.collection.clone()));
    body.push(BSON_ARRAY);
    body.extend_from_slice(b"documents");
    body.push(0);
    let docs = bson_document_pairs(&[("0".to_string(),
        Value::Object(doc_pairs.into_iter().collect()))]);
    body.extend_from_slice(&docs);
    put_bson_element(&mut body, "$db", &Value::String(cfg.database.clone()));
    body.push(0);
    let mut cmd = Vec::with_capacity(body.len() + 4);
    cmd.extend_from_slice(&((body.len() as i32) + 4).to_le_bytes());
    cmd.extend(body);
    cmd
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    if cfg.database.is_empty() || cfg.collection.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "mongodb exporter requires database + collection".into(),
        ));
    }
    let bson_body = build_body(fields, cfg);
    if bson_body.is_empty() { return Ok(()); }
    let frame = build_op_msg(&bson_body);

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_write_timeout(Some(timeout))?;
    s.write_all(&frame)?;
    s.flush()?;
    Ok(())
}
