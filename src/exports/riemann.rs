//! Riemann exporter — TCP binary protobuf-framed events. PARTIAL: encodes
//! a single `Event` message with `service`, `metric`, and `time` fields
//! using manual varint + length-prefix framing per Riemann's TCP spec.
//! Riemann also accepts msgpack / Thrift; both are out of scope.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

pub const NAME: &str = "riemann";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 5555,
            timeout_secs: 5,
        }
    }
}

/// Encode an unsigned varint (LEB128).
fn put_varint(buf: &mut Vec<u8>, mut n: u64) {
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 { buf.push(b); break; }
        buf.push(b | 0x80);
    }
}

/// Encode a protobuf field tag (wire type = 2 for length-delimited).
fn put_tag(buf: &mut Vec<u8>, field_number: u32) {
    put_varint(buf, ((field_number as u64) << 3) | 2);
}

fn put_string_field(buf: &mut Vec<u8>, field_number: u32, s: &str) {
    put_tag(buf, field_number);
    let b = s.as_bytes();
    put_varint(buf, b.len() as u64);
    buf.extend_from_slice(b);
}

fn put_double_field(buf: &mut Vec<u8>, field_number: u32, v: f64) {
    // wire type 1 = fixed 64-bit
    let tag = ((field_number as u64) << 3) | 1;
    put_varint(buf, tag);
    buf.extend_from_slice(&v.to_bits().to_le_bytes());
}

fn put_sint64_field(buf: &mut Vec<u8>, field_number: u32, v: i64) {
    // wire type 0 = varint
    let tag = ((field_number as u64) << 3) | 0;
    put_varint(buf, tag);
    put_varint(buf, v as u64);
}

/// Build one Riemann `Event` protobuf message. Exposed for unit tests.
/// Field numbers match Riemann's `Event` schema:
///   service = 1 (string), state = 2 (string), time = 4 (sint64),
///   metric_f = 6 (double), metric_d = 7 (sint64), ttl = 8 (double).
pub fn build_event(service: &str, metric: f64, time_ms: i64) -> Vec<u8> {
    let mut msg = Vec::new();
    put_string_field(&mut msg, 1, service);          // service
    put_string_field(&mut msg, 2, "ok");            // state
    put_sint64_field(&mut msg, 4, time_ms);         // time (ms)
    put_double_field(&mut msg, 6, metric);          // metric_f
    put_double_field(&mut msg, 8, 60.0);            // ttl (seconds)
    msg
}

pub(crate) fn build_message(events: &[Vec<u8>]) -> Vec<u8> {
    // Riemann TCP framing: each message is `i32 length || protobuf Msg`.
    // Msg { events = 1 (repeated Event) }
    let mut msg = Vec::new();
    for ev in events {
        put_tag(&mut msg, 1);
        put_varint(&mut msg, ev.len() as u64);
        msg.extend_from_slice(ev);
    }
    let mut framed = Vec::with_capacity(msg.len() + 4);
    framed.extend_from_slice(&(msg.len() as i32).to_be_bytes());
    framed.extend_from_slice(&msg);
    framed
}

pub(crate) fn build_payload(snap: &Value, now_ms: i64) -> Vec<u8> {
    let plugins = match snap.as_object() { Some(o) => o, None => return Vec::new() };
    let mut events = Vec::new();
    for (plugin, value) in plugins {
        let fields = match value.as_object() { Some(o) => o, None => continue };
        for (k, v) in fields {
            let n = match v {
                Value::Int(i) => Some(*i as f64),
                Value::Uint(u) => Some(*u as f64),
                Value::Float(f) if f.is_nan() || f.is_infinite() => None,
                Value::Float(f) => Some(*f),
                _ => None,
            };
            if let Some(n) = n {
                let service = format!("{}.{}", plugin, k);
                events.push(build_event(&service, n, now_ms));
            }
        }
    }
    build_message(&events)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    let payload = build_payload(snap, now_ms());
    if payload.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    let mut s = stream;
    s.write_all(&payload)?;
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
    fn varint_encodes_small_numbers_one_byte() {
        let mut buf = Vec::new();
        put_varint(&mut buf, 0);
        put_varint(&mut buf, 1);
        put_varint(&mut buf, 127);
        assert_eq!(buf, vec![0, 1, 0x7F]);
    }

    #[test]
    fn varint_handles_multi_byte_values() {
        let mut buf = Vec::new();
        put_varint(&mut buf, 128);
        put_varint(&mut buf, 300);
        // 128 = 0x80, 0x01; 300 = 0xAC, 0x02
        assert_eq!(buf, vec![0x80, 0x01, 0xAC, 0x02]);
    }

    #[test]
    fn event_contains_service_string() {
        let ev = build_event("cpu.total", 42.0, 1_700_000_000_000);
        let needle = b"cpu.total";
        assert!(ev.windows(needle.len()).any(|w| w == needle),
            "service string missing in: {:?}", ev);
    }

    #[test]
    fn message_is_length_prefixed() {
        let ev = build_event("x", 1.0, 0);
        let msg = build_message(&[ev.clone()]);
        let declared = i32::from_be_bytes([msg[0], msg[1], msg[2], msg[3]]);
        assert_eq!(declared as usize, msg.len() - 4);
    }

    #[test]
    fn nan_and_inf_metrics_skipped() {
        let snap = obj(&[(
            "cpu",
            obj(&[
                ("bad", Value::Float(f64::NAN)),
                ("good", Value::Int(1)),
            ]),
        )]);
        let payload = build_payload(&snap, 0);
        let raw = String::from_utf8_lossy(&payload);
        assert!(!raw.contains("bad"));
        assert!(raw.contains("good"));
    }
}