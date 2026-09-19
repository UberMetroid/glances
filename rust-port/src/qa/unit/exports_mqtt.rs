//! Unit tests for the MQTT 3.1.1 exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::mqtt;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn varint_encodes_remaining_length_correctly() {
    // Spec examples.
    assert_eq!(mqtt::encode_remaining_length(0), vec![0x00]);
    assert_eq!(mqtt::encode_remaining_length(127), vec![0x7F]);
    assert_eq!(mqtt::encode_remaining_length(128), vec![0x80, 0x01]);
    assert_eq!(mqtt::encode_remaining_length(16_383), vec![0xFF, 0x7F]);
    assert_eq!(mqtt::encode_remaining_length(16_384), vec![0x80, 0x80, 0x01]);
}

#[test]
fn connect_packet_starts_with_type_byte_and_embeds_protocol_name() {
    let cfg = mqtt::Config::default();
    let pkt = mqtt::build_connect(&cfg);
    assert_eq!(pkt[0], 0x10); // CONNECT type
    let raw = String::from_utf8_lossy(&pkt);
    let i = raw.find("MQTT").expect("MQTT marker present");
    // Protocol level for 3.1.1.
    assert_eq!(pkt[i + 4], 4);
    // Clean session flag bit must be set.
    assert_eq!(pkt[i + 5] & 0x02, 0x02);
}

#[test]
fn connect_with_credentials_sets_user_and_pass_flags() {
    let cfg = mqtt::Config {
        username: Some("u".into()),
        password: Some("p".into()),
        ..Default::default()
    };
    let pkt = mqtt::build_connect(&cfg);
    let raw = String::from_utf8_lossy(&pkt);
    let i = raw.find("MQTT").expect("MQTT marker present");
    // clean_session(0x02) | username(0x80) | password(0x40) = 0xC2.
    assert_eq!(pkt[i + 5], 0xC2);
}

#[test]
fn publish_carries_topic_with_prefix_and_qos_bits() {
    let cfg = mqtt::Config { qos: 1, ..Default::default() };
    let pkt = mqtt::build_publish(&cfg, "cpu", "total", b"42");
    // Header byte = 0x30 | (qos << 1) = 0x32.
    assert_eq!(pkt[0], 0x32);
    let raw = String::from_utf8_lossy(&pkt);
    assert!(raw.contains("glances/cpu/total"));
}

#[test]
fn empty_client_id_rejected() {
    let cfg = mqtt::Config { client_id: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = mqtt::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}