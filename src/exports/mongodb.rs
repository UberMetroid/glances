//! MongoDB OP_MSG exporter — binary `OP_MSG` request over TCP. PARTIAL:
//! builds a minimal `OP_MSG` frame containing a single BSON document with
//! plugin/key/value fields and the `insert` command. MongoDB requires a
//! `hello/isMaster` handshake first; this exporter does not perform it,
//! so a real mongod will close the connection after the first OP_MSG.
//! The wire framing (length-prefixed message header, OP_MSG opcode,
//! flag bits, BSON body) is exercised end-to-end.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

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
const BSON_BOOL: u8 = 0x08;
const BSON_INT32: u8 = 0x10;
const BSON_INT64: u8 = 0x12;
const BSON_NULL: u8 = 0x0A;

/// Append a BSON element. Returns the bytes appended.
fn put_bson_element(buf: &mut Vec<u8>, key: &str, v: &Value) {
    let key_bytes = key.as_bytes();
    // Element header = type (1 byte) + cstring key.
    match v {
        Value::Int(i) => {
            buf.push(BSON_INT32);
            buf.extend_from_slice(key_bytes);
            buf.push(0);
            buf.extend_from_slice(&(*i as i32).to_le_bytes());
        }
        Value::Uint(u) => {
            buf.push(BSON_INT64);
            buf.extend_from_slice(key_bytes);
            buf.push(0);
            buf.extend_from_slice(&(*u as i64).to_le_bytes());
        }
        Value::Float(f) if f.is_nan() || f.is_infinite() => {
            buf.push(BSON_NULL);
            buf.extend_from_slice(key_bytes);
            buf.push(0);
        }
        Value::Float(f) => {
            buf.push(BSON_DOUBLE);
            buf.extend_from_slice(key_bytes);
            buf.push(0);
            buf.extend_from_slice(&f.to_le_bytes());
        }
        Value::Bool(b) => {
            buf.push(BSON_BOOL);
            buf.extend_from_slice(key_bytes);
            buf.push(0);
            buf.push(if *b { 1 } else { 0 });
        }
        Value::String(s) => {
            buf.push(BSON_STRING);
            buf.extend_from_slice(key_bytes);
            buf.push(0);
            let bytes = s.as_bytes();
            buf.extend_from_slice(&((bytes.len() as i32) + 1).to_le_bytes());
            buf.extend_from_slice(bytes);
            buf.push(0); // NUL terminator
        }
        Value::Null | Value::Array(_) | Value::Object(_) => {
            buf.push(BSON_NULL);
            buf.extend_from_slice(key_bytes);
            buf.push(0);
        }
    }
}

/// Serialize a BSON document from an ordered key/value slice.
pub fn bson_document(pairs: &[(&str, Value)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (k, v) in pairs {
        put_bson_element(&mut body, k, v);
    }
    body.push(0); // document terminator
    let mut doc = Vec::with_capacity(body.len() + 4);
    doc.extend_from_slice(&((body.len() as i32) + 4).to_le_bytes());
    doc.extend(body);
    doc
}

/// Build the OP_MSG request frame (message header + flag bits + body BSON).
pub fn build_op_msg(bson_body: &[u8]) -> Vec<u8> {
    // Section 0 body = single BSON document + 0x00 section kind.
    let mut body_section = Vec::with_capacity(bson_body.len() + 1);
    body_section.push(0u8); // section kind 0 = body
    body_section.extend_from_slice(bson_body);

    let mut msg = Vec::with_capacity(16 + 4 + body_section.len());
    // MessageHeader: 16 bytes.
    let msg_length = (16 + 4 + body_section.len()) as i32;
    msg.extend_from_slice(&msg_length.to_le_bytes()); // messageLength
    msg.extend_from_slice(&1i32.to_le_bytes());       // requestID
    msg.extend_from_slice(&0i32.to_le_bytes());       // responseTo
    msg.extend_from_slice(&OP_MSG_OPCODE.to_le_bytes()); // opCode
    // OP_MSG-specific: flag bits + body.
    msg.extend_from_slice(&FLAG_NONE.to_le_bytes());
    msg.extend_from_slice(&body_section);
    msg
}

pub(crate) fn build_body(snap: &Value, cfg: &Config) -> Vec<u8> {
    // Flatten the snapshot into a single insert command document.
    let plugins = match snap.as_object() {
        Some(o) if !o.is_empty() => o,
        _ => return Vec::new(),
    };
    // Pick the first (plugin, key, value) we find — multi-doc inserts
    // would need OP_MSG with multiple sections; the partial stub keeps
    // things to a single body section.
    let (plugin, p_val) = plugins.iter().next().unwrap();
    let fields = match p_val.as_object() { Some(o) => o, None => return Vec::new() };
    let (key, value) = match fields.iter().next() {
        Some(t) => t,
        None => return Vec::new(),
    };
    let pairs: Vec<(&str, Value)> = vec![
        ("insert", Value::String(cfg.collection.clone())),
        ("database", Value::String(cfg.database.clone())),
        ("plugin", Value::String(plugin.clone())),
        ("key", Value::String(key.clone())),
        ("value", value.clone()),
    ];
    bson_document(&pairs)
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.database.is_empty() || cfg.collection.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "mongodb exporter requires database + collection".into(),
        ));
    }
    let bson_body = build_body(snap, cfg);
    if bson_body.is_empty() { return Ok(()); }
    let frame = build_op_msg(&bson_body);

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    let mut s = stream;
    s.write_all(&frame)?;
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
    fn bson_document_is_length_prefixed_and_terminated() {
        let doc = bson_document(&[("x", Value::Int(1))]);
        let declared = i32::from_le_bytes([doc[0], doc[1], doc[2], doc[3]]);
        assert_eq!(declared as usize, doc.len());
        assert_eq!(doc[doc.len() - 1], 0u8);
    }

    #[test]
    fn bson_string_carries_length_plus_nul() {
        let doc = bson_document(&[("k", Value::String("hi".into()))]);
        // doc layout: [i32 total][u8 type=0x02][k \0][i32 str_len=3][h i \0][0]
        // str_len = 2 (hi) + 1 (NUL) = 3.
        let needle: Vec<u8> = vec![0x02, b'k', 0, 3, 0, 0, 0, b'h', b'i', 0];
        assert!(doc.windows(needle.len()).any(|w| w == needle), "doc was: {:?}", doc);
    }

    #[test]
    fn op_msg_header_has_correct_opcode() {
        let bson_body = bson_document(&[("insert", Value::String("stats".into()))]);
        let msg = build_op_msg(&bson_body);
        // messageLength: bytes 0..4, requestID: 4..8, responseTo: 8..12,
        // opCode: 12..16.
        let opcode = i32::from_le_bytes([msg[12], msg[13], msg[14], msg[15]]);
        assert_eq!(opcode, OP_MSG_OPCODE);
        let declared = i32::from_le_bytes([msg[0], msg[1], msg[2], msg[3]]);
        assert_eq!(declared as usize, msg.len());
    }

    #[test]
    fn nan_floats_render_as_bson_null() {
        let doc = bson_document(&[("v", Value::Float(f64::NAN))]);
        // 0x0A = null type tag
        assert!(doc.windows(2).any(|w| w == [0x0A, b'v']), "doc was: {:?}", doc);
    }

    #[test]
    fn empty_database_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { database: String::new(), ..Default::default() };
        let err = write(&snap, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}