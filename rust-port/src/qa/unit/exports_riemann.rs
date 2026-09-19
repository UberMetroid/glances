//! Unit tests for the Riemann TCP exporter (protobuf-framed events).

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::riemann;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn event_message_contains_service_string() {
    let ev = riemann::build_event("cpu.total", 42.0, 1_700_000_000_000);
    let needle = b"cpu.total";
    assert!(ev.windows(needle.len()).any(|w| w == needle),
        "service string missing in event: {:?}", ev);
}

#[test]
fn message_is_length_prefixed_for_tcp_framing() {
    let ev = riemann::build_event("x", 1.0, 0);
    let msg = riemann::build_message(&[ev.clone()]);
    let declared = i32::from_be_bytes([msg[0], msg[1], msg[2], msg[3]]);
    assert_eq!(declared as usize, msg.len() - 4);
}

#[test]
fn nan_and_inf_metric_values_are_skipped() {
    let snap = obj(&[(
        "cpu",
        obj(&[
            ("bad_nan", Value::Float(f64::NAN)),
            ("bad_inf", Value::Float(f64::INFINITY)),
            ("good", Value::Int(1)),
        ]),
    )]);
    let payload = riemann::build_payload(&snap, 0);
    // Service name "good" should be present, "bad_*" should not appear
    // as a service string in the protobuf.
    let raw = payload.clone();
    assert!(!raw.windows(7).any(|w| w == b"bad_nan"));
    assert!(!raw.windows(7).any(|w| w == b"bad_inf"));
    assert!(raw.windows(4).any(|w| w == b"good"));
}

#[test]
fn empty_snapshot_yields_empty_payload() {
    let snap = Value::Object(BTreeMap::new());
    let payload = riemann::build_payload(&snap, 0);
    // Length prefix is 4 bytes for an empty body section.
    assert_eq!(payload.len(), 4);
    let declared = i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
    assert_eq!(declared, 0);
}

#[test]
fn default_port_is_riemann_standard() {
    assert_eq!(riemann::Config::default().port, 5555);
}