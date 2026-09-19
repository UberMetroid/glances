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

/// Walk the protobuf fields of one Event and return (field_no, wire_type).
fn field_tags(msg: &[u8]) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < msg.len() {
        // Tags here are all single-byte varints (fields 1,2,3,8,15).
        let tag = msg[i] as u64;
        i += 1;
        let (field, wire) = (tag >> 3, tag & 7);
        out.push((field, wire));
        i += match wire {
            0 => { while msg[i] & 0x80 != 0 { i += 1; } 1 } // varint
            1 => 8,                                        // fixed64
            2 => { let l = msg[i] as usize; 1 + l }        // LEN (len<128 here)
            5 => 4,                                        // fixed32
            _ => panic!("unexpected wire type {}", wire),
        };
    }
    out
}

#[test]
fn event_field_numbers_match_riemann_proto() {
    // riemann/proto/event.proto: time=1 int64, state=2, service=3,
    // ttl=8 float, metric_d=15 double. Regression: the old code used
    // 1/4/7/6/8 with wrong types and millisecond timestamps.
    let ev = riemann::build_event("svc", 1.0, 1_700_000_000);
    let tags = field_tags(&ev);
    let fields: Vec<u64> = tags.iter().map(|t| t.0).collect();
    assert_eq!(fields, vec![1, 2, 3, 8, 15]);
    assert_eq!(tags[0], (1, 0));  // time: varint (int64)
    assert_eq!(tags[1], (2, 2));  // state: LEN string
    assert_eq!(tags[2], (3, 2));  // service: LEN string
    assert_eq!(tags[3], (8, 5));  // ttl: fixed32 (float)
    assert_eq!(tags[4], (15, 1)); // metric_d: fixed64 (double)
}

#[test]
fn event_time_is_seconds_not_millis() {
    // int64 varint of 1_700_000_000 (seconds) fits in 5 bytes; the old
    // code multiplied by 1000 into a 6-byte varint.
    let ev = riemann::build_event("s", 0.0, 1_700_000_000);
    assert_eq!(ev[0], 0x08); // field 1, varint
    let mut v: u64 = 0;
    let mut shift = 0;
    let mut i = 1;
    loop {
        let b = ev[i];
        i += 1;
        v |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 { break; }
        shift += 7;
    }
    assert_eq!(v, 1_700_000_000);
}