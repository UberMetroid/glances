//! Unit tests for the RabbitMQ AMQP 0-9-1 exporter.

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::rabbitmq;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
}

#[test]
fn start_ok_frame_carries_plain_auth_on_channel_0() {
    let cfg = rabbitmq::Config {
        username: "u".into(),
        password: "p".into(),
        ..Default::default()
    };
    let f = rabbitmq::build_start_ok(&cfg);
    // Frame header: type 1 (method), channel 0, size, then
    // class 10 (connection) / method 11 (start-ok).
    assert_eq!(f[0], 1);
    assert_eq!(u16::from_be_bytes([f[1], f[2]]), 0);
    assert_eq!(u16::from_be_bytes([f[7], f[8]]), 10);
    assert_eq!(u16::from_be_bytes([f[9], f[10]]), 11);
    let raw = String::from_utf8_lossy(&f);
    assert!(raw.contains("PLAIN"));
    assert!(raw.contains("\0u\0p")); // PLAIN response: \0user\0pass
    assert_eq!(f[f.len() - 1], 0xCE); // frame-end marker
}

#[test]
fn publish_frame_is_method_on_channel_1() {
    let cfg = rabbitmq::Config::default();
    let frame = rabbitmq::build_publish_frame(&cfg);
    assert_eq!(frame[0], 1); // FRAME_METHOD
    let channel = u16::from_be_bytes([frame[1], frame[2]]);
    assert_eq!(channel, 1);
    // Last byte is the 0xCE frame-end marker.
    assert_eq!(frame[frame.len() - 1], 0xCE);
}

#[test]
fn publish_frame_carries_basic_publish_class_method_and_dest() {
    let cfg = rabbitmq::Config { exchange: "x".into(), routing_key: "y".into(), ..Default::default() };
    let frame = rabbitmq::build_publish_frame(&cfg);
    // size (4 bytes BE) starts at index 3.
    let body_len = u32::from_be_bytes([frame[3], frame[4], frame[5], frame[6]]) as usize;
    let body = &frame[7..7 + body_len];
    let cls = u16::from_be_bytes([body[0], body[1]]);
    let mth = u16::from_be_bytes([body[2], body[3]]);
    assert_eq!(cls, 60); // CLASS_BASIC
    assert_eq!(mth, 40); // METHOD_PUBLISH
    // args: reserved-1 empty shortstr, then "x"/"y" shortstrs.
    assert_eq!(body[4], 0);
    assert_eq!(&body[5..7], &[1, b'x']);
    assert_eq!(&body[7..9], &[1, b'y']);
}

#[test]
fn empty_snapshot_connects_to_nothing() {
    // No fields → write() returns Ok before resolving the host — an
    // unroutable port proves no connection is attempted.
    let cfg = rabbitmq::Config { port: 1, ..Default::default() };
    let snap = Value::Object(BTreeMap::new());
    rabbitmq::write(&flat(&snap), &cfg).expect("empty field set is a no-op");
}

#[test]
fn empty_vhost_rejected() {
    let cfg = rabbitmq::Config { vhost: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = rabbitmq::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn content_header_carries_body_size() {
    let f = rabbitmq::build_content_header(1234);
    assert_eq!(f[0], rabbitmq::FRAME_HEADER);
    let body_len = u64::from_be_bytes(f[11..19].try_into().unwrap());
    assert_eq!(body_len, 1234);
}

#[test]
fn doc_json_includes_series_and_elem() {
    let mut nic = BTreeMap::new();
    nic.insert("iface".into(), Value::String("eth0".into()));
    nic.insert("rx".into(), Value::Uint(9));
    let snap = obj(&[("network", Value::Array(vec![Value::Object(nic)]))]);
    let mut keys = HashMap::new();
    keys.insert("network".to_string(), "iface");
    let f = collect(&snap, &keys);
    let doc = String::from_utf8(rabbitmq::doc_json(&f[0])).unwrap();
    assert!(doc.contains("\"series\":\"network.eth0\""), "got: {}", doc);
    assert!(doc.contains("\"value\":9"));
}
