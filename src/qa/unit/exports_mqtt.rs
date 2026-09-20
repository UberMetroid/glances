//! Unit tests for the MQTT 3.1.1 exporter.

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::mqtt;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
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
fn qos_2_is_clamped_to_1() {
    // QoS 2 needs the PUBREC/PUBREL/PUBCOMP handshake the exporter
    // does not implement — it must never emit 0x34.
    let cfg = mqtt::Config { qos: 2, ..Default::default() };
    let pkt = mqtt::build_publish(&cfg, "cpu", "total", b"42");
    assert_eq!(pkt[0], 0x32);
}

#[test]
fn topic_wildcards_and_spaces_are_sanitized() {
    let cfg = mqtt::Config::default();
    let pkt = mqtt::build_publish(&cfg, "cp+u", "t#tal", b"1");
    let raw = String::from_utf8_lossy(&pkt);
    assert!(raw.contains("glances/cp_u/t_tal"), "got: {}", raw);
}

#[test]
fn empty_client_id_rejected() {
    let cfg = mqtt::Config { client_id: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = mqtt::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

/// Spawn a one-shot mock broker that reads the CONNECT packet, replies
/// with the given CONNACK bytes, then keeps reading (the client sends
/// its PUBLISH packets after CONNACK — closing early would RST the
/// client's second write and make this test racy).
fn mock_broker(connack: [u8; 4]) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
            let _ = s.write_all(&connack);
            let _ = s.flush();
            // Drain until the client disconnects (or we time out).
            while s.read(&mut buf).unwrap_or(0) > 0 {}
        }
    });
    port
}

#[test]
fn connack_success_is_accepted() {
    // Regression: CONNACK is 4 bytes; reading only 2 reported every
    // connection as rejected (ack[1] = remaining length 0x02).
    let port = mock_broker([0x20, 0x02, 0x00, 0x00]);
    let cfg = mqtt::Config { port, ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    assert!(mqtt::write(&flat(&snap), &cfg).is_ok(), "CONNACK rc=0 must succeed");
}

#[test]
fn connack_nonzero_return_code_is_rejected() {
    let port = mock_broker([0x20, 0x02, 0x00, 0x05]); // 5 = not authorized
    let cfg = mqtt::Config { port, ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = mqtt::write(&flat(&snap), &cfg).unwrap_err();
    assert!(err.to_string().contains("5"));
}
