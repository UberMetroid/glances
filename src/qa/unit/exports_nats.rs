//! Unit tests for the NATS text-protocol exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::nats;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn pub_command_has_subject_and_payload_size() {
    let cmd = nats::build_pub("glances", "cpu", "total", b"42");
    let raw = String::from_utf8(cmd).unwrap();
    assert_eq!(raw, "PUB glances.cpu.total 0 2\r\n42\r\n");
}

#[test]
fn pub_handles_empty_payload() {
    let cmd = nats::build_pub("p", "a", "b", b"");
    let raw = String::from_utf8(cmd).unwrap();
    assert_eq!(raw, "PUB p.a.b 0 0\r\n\r\n");
}

#[test]
fn publish_set_emits_one_pub_per_field() {
    let snap = obj(&[(
        "cpu",
        obj(&[("x", Value::Int(1)), ("y", Value::Int(2))]),
    )]);
    let bytes = nats::build_publishes(&snap, "gl");
    let raw = String::from_utf8(bytes).unwrap();
    assert!(raw.contains("PUB gl.cpu.x 0 1\r\n1\r\n"));
    assert!(raw.contains("PUB gl.cpu.y 0 1\r\n2\r\n"));
}

#[test]
fn empty_subject_prefix_rejected() {
    let cfg = nats::Config { subject_prefix: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = nats::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn default_port_is_nats_standard() {
    let cfg = nats::Config::default();
    assert_eq!(cfg.port, 4222);
    assert_eq!(cfg.host, "127.0.0.1");
    assert_eq!(cfg.subject_prefix, "glances");
}