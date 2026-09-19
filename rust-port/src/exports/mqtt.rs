//! MQTT 3.1.1 exporter — binary CONNECT + PUBLISH over TCP. Each refresh
//! tick opens a fresh TCP connection, sends a CONNECT packet, then one
//! PUBLISH packet per (plugin, key) tuple, and closes. The broker will
//! reply with CONNACK; we read it for protocol hygiene but do not act
//! on its return code beyond failing the call on a non-zero value.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "mqtt";

const MQTT_PROTOCOL_LEVEL: u8 = 4; // 3.1.1

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub topic_prefix: String,
    pub qos: u8,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 1883,
            client_id: "glances-rs".into(),
            username: None,
            password: None,
            topic_prefix: "glances".into(),
            qos: 0,
            timeout_secs: 5,
        }
    }
}

/// Encode an MQTT variable-length integer (max 4 bytes per spec).
pub fn encode_remaining_length(len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut x = len;
    loop {
        let mut byte = (x & 0x7F) as u8;
        x >>= 7;
        if x > 0 { byte |= 0x80; }
        out.push(byte);
        if x == 0 { break; }
    }
    out
}

/// Build the CONNECT packet: variable header + payload.
pub fn build_connect(cfg: &Config) -> Vec<u8> {
    let body = build_connect_inner(cfg);
    let mut pkt = Vec::with_capacity(body.len() + 5);
    pkt.push(0x10); // CONNECT packet type
    pkt.extend_from_slice(&encode_remaining_length(body.len()));
    pkt.extend_from_slice(&body);
    pkt
}

fn build_connect_inner(cfg: &Config) -> Vec<u8> {
    let mut buf = Vec::new();
    // Protocol name "MQTT" (length-prefixed UTF-8).
    buf.extend_from_slice(&[0x00, 0x04]);
    buf.extend_from_slice(b"MQTT");
    buf.push(MQTT_PROTOCOL_LEVEL);

    let mut flags: u8 = 0x02; // Clean Session
    if cfg.username.is_some() { flags |= 0x80; }
    if cfg.password.is_some() { flags |= 0x40; }
    buf.push(flags);
    buf.extend_from_slice(&60u16.to_be_bytes()); // Keep Alive = 60s

    let cid = cfg.client_id.as_bytes();
    buf.extend_from_slice(&(cid.len() as u16).to_be_bytes());
    buf.extend_from_slice(cid);
    if let Some(u) = cfg.username.as_ref() {
        let b = u.as_bytes();
        buf.extend_from_slice(&(b.len() as u16).to_be_bytes());
        buf.extend_from_slice(b);
    }
    if let Some(p) = cfg.password.as_ref() {
        let b = p.as_bytes();
        buf.extend_from_slice(&(b.len() as u16).to_be_bytes());
        buf.extend_from_slice(b);
    }
    buf
}

/// Build a PUBLISH packet for one (plugin, key, value) triple.
pub fn build_publish(cfg: &Config, plugin: &str, key: &str, payload: &[u8]) -> Vec<u8> {
    let topic = format!("{}/{}/{}", cfg.topic_prefix, plugin, key);
    let topic_bytes = topic.as_bytes();
    let qos = cfg.qos.min(2);

    let mut body = Vec::new();
    body.extend_from_slice(&(topic_bytes.len() as u16).to_be_bytes());
    body.extend_from_slice(topic_bytes);
    if qos > 0 {
        body.extend_from_slice(&1u16.to_be_bytes()); // Packet Identifier = 1
    }
    body.extend_from_slice(payload);

    let mut pkt = Vec::with_capacity(body.len() + 5);
    // PUBLISH type (3) << 4 | qos flags in bits 1-2.
    pkt.push(0x30 | ((qos & 0x03) << 1));
    pkt.extend_from_slice(&encode_remaining_length(body.len()));
    pkt.extend_from_slice(&body);
    pkt
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

pub(crate) fn build_publishes(snap: &Value, cfg: &Config) -> Vec<u8> {
    let plugins = match snap.as_object() { Some(o) => o, None => return Vec::new() };
    let mut out = Vec::new();
    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        for (k, v) in fields {
            let payload = render_payload(v);
            out.extend_from_slice(&build_publish(cfg, plugin, k, &payload));
        }
    }
    out
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    if cfg.client_id.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "mqtt exporter requires client_id".into(),
        ));
    }
    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_read_timeout(Some(timeout))?;

    stream.write_all(&build_connect(cfg))?;
    stream.flush()?;
    let mut ack = [0u8; 2];
    stream.read_exact(&mut ack)?;
    if ack[1] != 0 {
        return Err(GlancesError::Other(format!(
            "{} broker rejected CONNECT (return code {})", NAME, ack[1],
        )));
    }

    let publishes = build_publishes(snap, cfg);
    if !publishes.is_empty() {
        stream.write_all(&publishes)?;
        stream.flush()?;
    }
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
    fn remaining_length_varint_encoding() {
        assert_eq!(encode_remaining_length(0), vec![0]);
        assert_eq!(encode_remaining_length(127), vec![0x7F]);
        assert_eq!(encode_remaining_length(128), vec![0x80, 0x01]);
        assert_eq!(encode_remaining_length(16383), vec![0xFF, 0x7F]);
    }

    #[test]
    fn connect_packet_starts_with_type_byte() {
        let cfg = Config::default();
        let pkt = build_connect(&cfg);
        assert_eq!(pkt[0], 0x10);
    }

    #[test]
    fn connect_carries_clean_session_and_protocol_level() {
        let cfg = Config::default();
        let pkt = build_connect(&cfg);
        let raw = String::from_utf8_lossy(&pkt);
        let i = raw.find("MQTT").unwrap();
        assert_eq!(pkt[i + 4], MQTT_PROTOCOL_LEVEL);
        assert_eq!(pkt[i + 5] & 0x02, 0x02);
    }

    #[test]
    fn connect_with_user_pass_sets_flags() {
        let cfg = Config {
            username: Some("u".into()),
            password: Some("p".into()),
            ..Default::default()
        };
        let pkt = build_connect(&cfg);
        let raw = String::from_utf8_lossy(&pkt);
        let i = raw.find("MQTT").unwrap();
        // clean_session (0x02) | has_user (0x80) | has_pass (0x40) = 0xC2
        assert_eq!(pkt[i + 5], 0xC2);
    }

    #[test]
    fn publish_uses_topic_prefix_and_qos_bits() {
        let cfg = Config { qos: 1, ..Default::default() };
        let pkt = build_publish(&cfg, "cpu", "total", b"42");
        assert_eq!(pkt[0], 0x32);
        let raw = String::from_utf8_lossy(&pkt);
        assert!(raw.contains("glances/cpu/total"));
    }

    #[test]
    fn empty_client_id_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { client_id: String::new(), ..Default::default() };
        let err = write(&snap, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}