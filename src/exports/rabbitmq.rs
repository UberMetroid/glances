//! RabbitMQ AMQP 0-9-1 exporter — TCP binary protocol. PARTIAL: emits the
//! 8-byte protocol header `AMQP\x00\x00\x09\x01` to identify the client,
//! then a `basic.publish` frame (class 60 / method 40) carrying one
//! (plugin, key, value) JSON payload. The full Connection.Start /
//! Connection.Tune / Connection.Open handshake is out of scope; a real
//! broker will close the connection after the first unframed method.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "rabbitmq";

const AMQP_PROTOCOL_HEADER: &[u8; 8] = b"AMQP\x00\x00\x09\x01";
const CLASS_BASIC: u16 = 60;
const METHOD_PUBLISH: u16 = 40;
const FRAME_METHOD: u8 = 1;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub vhost: String,
    pub exchange: String,
    pub routing_key: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 5672,
            vhost: "/".into(),
            exchange: "amq.direct".into(),
            routing_key: "glances".into(),
            timeout_secs: 5,
        }
    }
}

/// Append a short-string (1-byte length + UTF-8 bytes).
fn put_short_string(buf: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    buf.push(b.len() as u8);
    buf.extend_from_slice(b);
}

/// Append a long-string (4-byte length + UTF-8 bytes).
fn put_long_string(buf: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    buf.extend_from_slice(&(b.len() as u32).to_be_bytes());
    buf.extend_from_slice(b);
}

/// Build the AMQP protocol header (the 8 bytes the client sends first).
pub fn protocol_header() -> Vec<u8> {
    AMQP_PROTOCOL_HEADER.to_vec()
}

/// Build one `basic.publish` method frame: type=1, channel=1, size,
/// payload (class + method + args), 0xCE frame-end byte.
pub fn build_publish_frame(cfg: &Config, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&CLASS_BASIC.to_be_bytes());
    body.extend_from_slice(&METHOD_PUBLISH.to_be_bytes());
    put_short_string(&mut body, &cfg.exchange);
    put_short_string(&mut body, &cfg.routing_key);
    body.push(0); // mandatory = false
    body.push(0); // immediate = false
    put_long_string(&mut body, std::str::from_utf8(payload).unwrap_or(""));

    let mut frame = Vec::new();
    frame.push(FRAME_METHOD);
    frame.extend_from_slice(&1u16.to_be_bytes()); // channel = 1
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    frame.push(0xCE); // frame-end
    frame
}

fn render_payload(value: &Value) -> Vec<u8> {
    match value {
        Value::Int(i) => i.to_string().into_bytes(),
        Value::Uint(u) => u.to_string().into_bytes(),
        Value::Float(f) if f.is_nan() || f.is_infinite() => b"null".to_vec(),
        Value::Float(f) => format!("{}", f).into_bytes(),
        Value::Bool(b) => b.to_string().into_bytes(),
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Null | Value::Array(_) | Value::Object(_) => b"null".to_vec(),
    }
}

pub(crate) fn build_frames(snap: &Value, cfg: &Config) -> Vec<u8> {
    let plugins = match snap.as_object() { Some(o) => o, None => return Vec::new() };
    let mut out = Vec::new();
    out.extend_from_slice(&protocol_header());
    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        for (k, v) in fields {
            let mut payload = format!("{}.{}=", plugin, k).into_bytes();
            payload.extend_from_slice(&render_payload(v));
            out.extend_from_slice(&build_publish_frame(cfg, &payload));
        }
    }
    out
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.vhost.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "rabbitmq exporter requires vhost".into(),
        ));
    }
    let bytes = build_frames(snap, cfg);
    if bytes.len() <= AMQP_PROTOCOL_HEADER.len() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    let mut s = stream;
    s.write_all(&bytes)?;
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
    fn protocol_header_is_eight_bytes() {
        let hdr = protocol_header();
        assert_eq!(hdr.len(), 8);
        assert_eq!(&hdr, b"AMQP\x00\x00\x09\x01");
    }

    #[test]
    fn publish_frame_starts_with_method_type_and_channel() {
        let cfg = Config::default();
        let frame = build_publish_frame(&cfg, b"42");
        assert_eq!(frame[0], FRAME_METHOD);
        assert_eq!(u16::from_be_bytes([frame[1], frame[2]]), 1);
        // Last byte is the 0xCE frame-end marker.
        assert_eq!(frame[frame.len() - 1], 0xCE);
    }

    #[test]
    fn publish_frame_carries_class_and_method() {
        let cfg = Config { exchange: "x".into(), routing_key: "y".into(), ..Default::default() };
        let frame = build_publish_frame(&cfg, b"42");
        // size (4 bytes BE) starts at index 3, then class + method.
        let body_len = u32::from_be_bytes([frame[3], frame[4], frame[5], frame[6]]) as usize;
        let body = &frame[7..7 + body_len];
        let cls = u16::from_be_bytes([body[0], body[1]]);
        let mth = u16::from_be_bytes([body[2], body[3]]);
        assert_eq!(cls, CLASS_BASIC);
        assert_eq!(mth, METHOD_PUBLISH);
    }

    #[test]
    fn empty_snapshot_writes_no_publish_frames() {
        let cfg = Config::default();
        let snap = Value::Object(BTreeMap::new());
        let bytes = build_frames(&snap, &cfg);
        // Only the protocol header.
        assert_eq!(bytes.len(), AMQP_PROTOCOL_HEADER.len());
    }

    #[test]
    fn empty_vhost_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { vhost: String::new(), ..Default::default() };
        let err = write(&snap, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}