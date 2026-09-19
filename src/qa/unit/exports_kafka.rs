//! Unit tests for the Kafka binary frame exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::kafka;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn frame_starts_with_big_endian_length_matching_body() {
    let cfg = kafka::Config::default();
    let frame = kafka::build_frame(&cfg, 1, b"{}").unwrap();
    assert!(frame.len() > 4);
    let declared = i32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
    assert_eq!(declared as usize, frame.len() - 4);
}

#[test]
fn request_header_carries_produce_api_key_and_version() {
    let cfg = kafka::Config::default();
    let frame = kafka::build_frame(&cfg, 7, b"{}").unwrap();
    let api_key = i16::from_be_bytes([frame[4], frame[5]]);
    let api_version = i16::from_be_bytes([frame[6], frame[7]]);
    let correlation = i32::from_be_bytes([frame[8], frame[9], frame[10], frame[11]]);
    assert_eq!(api_key, 0); // Produce
    assert_eq!(api_version, 0);
    assert_eq!(correlation, 7);
}

#[test]
fn topic_name_appears_in_frame() {
    let cfg = kafka::Config { topic: "cpu-metrics".into(), ..Default::default() };
    let frame = kafka::build_frame(&cfg, 1, b"{}").unwrap();
    let needle = b"cpu-metrics";
    let found = frame.windows(needle.len()).any(|w| w == needle);
    assert!(found, "topic name not found in frame");
}

#[test]
fn empty_topic_rejected() {
    let cfg = kafka::Config { topic: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = kafka::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn default_port_is_kafka_standard() {
    assert_eq!(kafka::Config::default().port, 9092);
}