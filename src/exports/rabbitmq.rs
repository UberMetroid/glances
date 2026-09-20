//! RabbitMQ AMQP 0-9-1 exporter — real client handshake over TCP:
//! protocol header → `connection.start`/`start-ok` (PLAIN auth) →
//! `connection.tune`/`tune-ok` → `connection.open` → `channel.open`,
//! then per field a `basic.publish` method frame + content header +
//! content body frames (chunked under the negotiated frame-max).
//!
//! The connection is opened and closed per refresh tick — fire and
//! forget, no publish-confirm wait (matching Python Glances' exporter,
//! which also does not confirm).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::{to_json, Value};
use crate::exports::flatten::Field;

pub const NAME: &str = "rabbitmq";

const PROTOCOL_HEADER: &[u8; 8] = b"AMQP\x00\x00\x09\x01";
pub(crate) const FRAME_METHOD: u8 = 1;
pub(crate) const FRAME_HEADER: u8 = 2;
const FRAME_BODY: u8 = 3;
const FRAME_END: u8 = 0xCE;
const CLASS_CONNECTION: u16 = 10;
const CLASS_CHANNEL: u16 = 20;
const CLASS_BASIC: u16 = 60;
/// Server frame payloads are bounded (connection.* frames are tiny).
const MAX_SERVER_FRAME: usize = 1 << 20;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub vhost: String,
    pub exchange: String,
    pub routing_key: String,
    pub username: String,
    pub password: String,
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
            username: "guest".into(),
            password: "guest".into(),
            timeout_secs: 5,
        }
    }
}

fn put_short_string(buf: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    buf.push(b.len() as u8);
    buf.extend_from_slice(b);
}
fn put_long_string(buf: &mut Vec<u8>, b: &[u8]) {
    buf.extend_from_slice(&(b.len() as u32).to_be_bytes());
    buf.extend_from_slice(b);
}

/// Generic frame: `type|channel|size|payload|0xCE`.
fn frame(ftype: u8, channel: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 8);
    f.push(ftype);
    f.extend_from_slice(&channel.to_be_bytes());
    f.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    f.extend_from_slice(payload);
    f.push(FRAME_END);
    f
}

/// Method frame: payload = `class|method|args`.
fn method_frame(channel: u16, class: u16, method: u16, args: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(args.len() + 4);
    p.extend_from_slice(&class.to_be_bytes());
    p.extend_from_slice(&method.to_be_bytes());
    p.extend_from_slice(args);
    frame(FRAME_METHOD, channel, &p)
}

/// `connection.start-ok` args: client-properties (empty field table) +
/// mechanism "PLAIN" + `\0user\0pass` response + locale "en_US".
pub fn build_start_ok(cfg: &Config) -> Vec<u8> {
    let mut a = Vec::new();
    a.extend_from_slice(&0u32.to_be_bytes()); // empty client-properties table
    put_long_string(&mut a, b"PLAIN");
    let mut resp = vec![0u8];
    resp.extend_from_slice(cfg.username.as_bytes());
    resp.push(0);
    resp.extend_from_slice(cfg.password.as_bytes());
    put_long_string(&mut a, &resp);
    put_short_string(&mut a, "en_US");
    method_frame(0, CLASS_CONNECTION, 11, &a)
}

/// `connection.tune-ok`: echo the server's channel/frame maxima, no heartbeat.
fn build_tune_ok(frame_max: u32) -> Vec<u8> {
    let mut a = Vec::new();
    a.extend_from_slice(&1u16.to_be_bytes());     // channel-max = 1
    a.extend_from_slice(&frame_max.to_be_bytes());
    a.extend_from_slice(&0u16.to_be_bytes());     // heartbeat disabled
    method_frame(0, CLASS_CONNECTION, 31, &a)
}

/// `connection.open`: vhost + empty capabilities + insist=false.
fn build_open(vhost: &str) -> Vec<u8> {
    let mut a = Vec::new();
    put_short_string(&mut a, vhost);
    put_short_string(&mut a, ""); // capabilities
    a.push(0);                    // insist = false
    method_frame(0, CLASS_CONNECTION, 40, &a)
}

/// `channel.open` on channel 1 (empty out-of-band string).
fn build_channel_open() -> Vec<u8> {
    let mut a = Vec::new();
    put_short_string(&mut a, "");
    method_frame(1, CLASS_CHANNEL, 10, &a)
}

/// `basic.publish` on channel 1: reserved-1 + exchange + routing-key +
/// mandatory/immediate bits = 0.
pub fn build_publish_frame(cfg: &Config) -> Vec<u8> {
    let mut a = Vec::new();
    put_short_string(&mut a, "");              // reserved-1 (must be empty)
    put_short_string(&mut a, &cfg.exchange);
    put_short_string(&mut a, &cfg.routing_key);
    a.push(0); // mandatory|immediate bitfield
    method_frame(1, CLASS_BASIC, 40, &a)
}

/// Content header frame: class 60, weight 0, body-size, property flags 0.
pub(crate) fn build_content_header(body_len: u64) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&CLASS_BASIC.to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());  // weight (unused)
    p.extend_from_slice(&body_len.to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());  // no content properties
    frame(FRAME_HEADER, 1, &p)
}

/// Read one server frame → `(ftype, channel, payload)`; verifies the
/// 0xCE end byte and bounds the payload size.
fn read_frame(s: &mut TcpStream) -> Result<(u8, u16, Vec<u8>)> {
    let mut head = [0u8; 7];
    s.read_exact(&mut head)?;
    let size = u32::from_be_bytes([head[3], head[4], head[5], head[6]]) as usize;
    if size > MAX_SERVER_FRAME {
        return Err(GlancesError::Parse(format!(
            "rabbitmq: server frame too large ({} bytes)", size,
        )));
    }
    let mut payload = vec![0u8; size];
    s.read_exact(&mut payload)?;
    let mut end = [0u8; 1];
    s.read_exact(&mut end)?;
    if end[0] != FRAME_END {
        return Err(GlancesError::Parse("rabbitmq: bad frame-end byte".into()));
    }
    Ok((head[0], u16::from_be_bytes([head[1], head[2]]), payload))
}

/// `(class, method)` of a method-frame payload, or None.
fn method_of(payload: &[u8]) -> (u16, u16) {
    if payload.len() < 4 { return (0, 0); }
    (u16::from_be_bytes([payload[0], payload[1]]),
     u16::from_be_bytes([payload[2], payload[3]]))
}

/// Expect a specific (class, method) server frame.
fn expect_method(s: &mut TcpStream, class: u16, method: u16) -> Result<Vec<u8>> {
    let (t, _ch, p) = read_frame(s)?;
    if t != FRAME_METHOD || method_of(&p) != (class, method) {
        return Err(GlancesError::Other(format!(
            "rabbitmq: expected {}.{}, got frame type {} method {:?}",
            class, method, t, method_of(&p),
        )));
    }
    Ok(p)
}

/// Small JSON doc per published metric: `{"plugin","series","key","value"}`.
pub(crate) fn doc_json(f: &Field<'_>) -> Vec<u8> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("plugin".to_string(), Value::String(f.plugin.to_string()));
    m.insert("series".to_string(), Value::String(f.series.clone()));
    m.insert("key".to_string(), Value::String(f.key.to_string()));
    m.insert("value".to_string(), f.value.clone());
    to_json(&Value::Object(m)).into_bytes()
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    if cfg.vhost.is_empty() || cfg.exchange.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "rabbitmq exporter requires vhost + exchange".into(),
        ));
    }
    if fields.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;
    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_write_timeout(Some(timeout))?;
    s.set_read_timeout(Some(timeout))?;

    // Handshake.
    s.write_all(PROTOCOL_HEADER)?;
    expect_method(&mut s, CLASS_CONNECTION, 10)?;          // start
    s.write_all(&build_start_ok(cfg))?;
    let tune = expect_method(&mut s, CLASS_CONNECTION, 30)?; // tune
    let frame_max = if tune.len() >= 8 {
        u32::from_be_bytes([tune[4], tune[5], tune[6], tune[7]])
    } else { 131_072 };
    let frame_max = if frame_max == 0 { 131_072 } else { frame_max };
    s.write_all(&build_tune_ok(frame_max))?;
    s.write_all(&build_open(&cfg.vhost))?;
    expect_method(&mut s, CLASS_CONNECTION, 41)?;          // open-ok
    s.write_all(&build_channel_open())?;
    expect_method(&mut s, CLASS_CHANNEL, 11)?;             // channel.open-ok

    // Publish one message per field.
    let publish = build_publish_frame(cfg);
    let chunk_max = (frame_max as usize).saturating_sub(8).max(1);
    for f in fields {
        let body = doc_json(f);
        s.write_all(&publish)?;
        s.write_all(&build_content_header(body.len() as u64))?;
        for chunk in body.chunks(chunk_max) {
            s.write_all(&frame(FRAME_BODY, 1, chunk))?;
        }
    }
    s.flush()?;
    Ok(())
}
