//! Unit tests for the OpenTSDB telnet-put exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::opentsdb;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn render_put_produces_put_command() {
    let line = opentsdb::render_put("cpu", "total", 42.0, 1_700_000_000).unwrap();
    assert_eq!(line, "put cpu.total 1700000000 42 plugin=cpu\n");
}

#[test]
fn render_put_skips_nan_and_inf() {
    assert!(opentsdb::render_put("cpu", "x", f64::NAN, 0).is_none());
    assert!(opentsdb::render_put("cpu", "x", f64::INFINITY, 0).is_none());
    assert!(opentsdb::render_put("cpu", "x", f64::NEG_INFINITY, 0).is_none());
}

#[test]
fn render_put_sanitizes_whitespace() {
    let line = opentsdb::render_put("cpu 0", "rx bytes", 1.0, 100).unwrap();
    // OpenTSDB does not allow spaces in metric/tag names.
    assert!(line.starts_with("put cpu_0.rx_bytes 100 1 plugin=cpu_0\n"));
}

#[test]
fn build_body_emits_one_put_per_numeric_field() {
    let snap = obj(&[(
        "cpu",
        obj(&[
            ("good", Value::Int(1)),
            ("bad", Value::Float(f64::NAN)),
            ("str", Value::String("hi".into())),
        ]),
    )]);
    let body = opentsdb::build_body(&snap, 100);
    assert!(body.contains("put cpu.good 100 1 plugin=cpu\n"));
    assert!(!body.contains("bad"));
    // String values are skipped (not numeric).
    assert!(!body.contains("str"));
}

#[test]
fn empty_snapshot_yields_empty_body() {
    let snap = Value::Object(BTreeMap::new());
    let body = opentsdb::build_body(&snap, 100);
    assert!(body.is_empty());
}

#[test]
fn default_port_is_opentsdb_standard() {
    assert_eq!(opentsdb::Config::default().port, 4242);
}