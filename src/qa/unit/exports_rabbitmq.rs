//! Unit tests for the RabbitMQ AMQP 0-9-1 exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::rabbitmq;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn protocol_header_is_eight_bytes_magic_string() {
    let hdr = rabbitmq::protocol_header();
    assert_eq!(hdr.len(), 8);
    assert_eq!(&hdr, b"AMQP\x00\x00\x09\x01");
}

#[test]
fn publish_frame_starts_with_method_type_and_channel() {
    let cfg = rabbitmq::Config::default();
    let frame = rabbitmq::build_publish_frame(&cfg, b"42");
    assert_eq!(frame[0], 1); // FRAME_METHOD
    let channel = u16::from_be_bytes([frame[1], frame[2]]);
    assert_eq!(channel, 1);
    // Last byte is the 0xCE frame-end marker.
    assert_eq!(frame[frame.len() - 1], 0xCE);
}

#[test]
fn publish_frame_carries_basic_publish_class_and_method() {
    let cfg = rabbitmq::Config { exchange: "x".into(), routing_key: "y".into(), ..Default::default() };
    let frame = rabbitmq::build_publish_frame(&cfg, b"42");
    // size (4 bytes BE) starts at index 3.
    let body_len = u32::from_be_bytes([frame[3], frame[4], frame[5], frame[6]]) as usize;
    let body = &frame[7..7 + body_len];
    let cls = u16::from_be_bytes([body[0], body[1]]);
    let mth = u16::from_be_bytes([body[2], body[3]]);
    assert_eq!(cls, 60); // CLASS_BASIC
    assert_eq!(mth, 40); // METHOD_PUBLISH
}

#[test]
fn empty_snapshot_writes_no_publish_frames() {
    let cfg = rabbitmq::Config::default();
    let snap = Value::Object(BTreeMap::new());
    let bytes = rabbitmq::build_frames(&snap, &cfg);
    // Just the protocol header.
    assert_eq!(bytes.len(), 8);
}

#[test]
fn empty_vhost_rejected() {
    let cfg = rabbitmq::Config { vhost: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = rabbitmq::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}