//! Kafka exporter — binary Produce request over TCP. PARTIAL: builds the
//! 4-byte length-prefixed request envelope with `api_key=0` (Produce) +
//! v0 body header (`acks`, `timeout`, `topic`, `partition`) but does not
//! serialize a complete MessageSet / RecordBatch v2. The wire-level
//! framing (length prefix, request header, string encoding) is exercised
//! end-to-end so a real broker rejects the frame deterministically rather
//! than panicking the client.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "kafka";

const PRODUCE_API_KEY: i16 = 0;
const PRODUCE_API_VERSION: i16 = 0;
const ACKS_FIRE_AND_FORGET: i16 = 0;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub topic: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9092,
            topic: "glances".into(),
            timeout_secs: 5,
        }
    }
}

/// Append an i16 big-endian.
fn put_i16(buf: &mut Vec<u8>, v: i16) {
    buf.extend_from_slice(&v.to_be_bytes());
}
/// Append an i32 big-endian.
fn put_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_be_bytes());
}
/// Append a Kafka string (i16 length + UTF-8 bytes).
fn put_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len();
    assert!(len <= i16::MAX as usize, "string too long for Kafka i16 length");
    put_i16(buf, len as i16);
    buf.extend_from_slice(bytes);
}

/// Build a length-prefixed Produce v0 request frame. The frame is
/// `i32 length || request_body`. The body itself contains the request
/// header and a topic + partition placeholder.
pub fn build_frame(cfg: &Config, correlation_id: i32) -> Vec<u8> {
    // Request body (everything except the 4-byte length prefix).
    let mut body = Vec::new();
    put_i16(&mut body, PRODUCE_API_KEY);
    put_i16(&mut body, PRODUCE_API_VERSION);
    put_i32(&mut body, correlation_id);
    put_string(&mut body, "glances-rs");
    // ProduceRequest v0 body:
    put_i16(&mut body, ACKS_FIRE_AND_FORGET);
    put_i32(&mut body, cfg.timeout_secs as i32);
    put_i32(&mut body, 1); // 1 topic
    put_string(&mut body, &cfg.topic);
    put_i32(&mut body, 1); // 1 partition
    put_i32(&mut body, 0); // partition 0
    put_i32(&mut body, 0); // empty MessageSet v0 — broker will reject, framing is correct
    // Prepend 4-byte length.
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as i32).to_be_bytes());
    frame.extend(body);
    frame
}

pub(crate) fn build_body(snap: &Value, topic: &str) -> Vec<u8> {
    // Snap-driven body is currently identical to the static frame; the
    // snap parameter is reserved so we can extend this exporter to embed
    // plugin/key/value triples in the MessageSet without breaking the
    // public signature.
    let _ = (snap, topic);
    Vec::new()
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.topic.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "kafka exporter requires topic".into(),
        ));
    }
    let _ = build_body(snap, &cfg.topic);
    let frame = build_frame(cfg, 1);

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
    fn frame_starts_with_big_endian_length() {
        let cfg = Config::default();
        let frame = build_frame(&cfg, 1);
        assert!(frame.len() > 4);
        let declared = i32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
        assert_eq!(declared as usize, frame.len() - 4);
    }

    #[test]
    fn frame_encodes_produce_request_header() {
        let cfg = Config { topic: "metrics".into(), ..Default::default() };
        let frame = build_frame(&cfg, 7);
        // Skip 4-byte length, then api_key (2), api_version (2), correlation_id (4).
        let api_key = i16::from_be_bytes([frame[4], frame[5]]);
        let api_version = i16::from_be_bytes([frame[6], frame[7]]);
        let correlation = i32::from_be_bytes([frame[8], frame[9], frame[10], frame[11]]);
        assert_eq!(api_key, PRODUCE_API_KEY);
        assert_eq!(api_version, PRODUCE_API_VERSION);
        assert_eq!(correlation, 7);
    }

    #[test]
    fn frame_contains_topic_name() {
        let cfg = Config { topic: "cpu-metrics".into(), ..Default::default() };
        let frame = build_frame(&cfg, 1);
        let needle = b"cpu-metrics";
        // Scan the frame for the topic bytes (Kafka strings are length-prefixed).
        let mut found = false;
        for w in frame.windows(needle.len()) {
            if w == needle { found = true; break; }
        }
        assert!(found, "topic name not found in frame");
    }

    #[test]
    fn empty_topic_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { topic: String::new(), ..Default::default() };
        let err = write(&snap, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}