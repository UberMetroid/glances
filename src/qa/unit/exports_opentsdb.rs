//! Unit tests for the OpenTSDB telnet-put exporter.

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::opentsdb;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
}

#[test]
fn render_put_produces_put_command() {
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(42.0))]))]);
    let fields = flat(&snap);
    let line = opentsdb::render_put(&fields[0], 42.0, 1_700_000_000).unwrap();
    assert_eq!(line, "put cpu.total 1700000000 42 plugin=cpu\n");
}

#[test]
fn render_put_skips_nan_and_inf() {
    let snap = obj(&[("cpu", obj(&[("x", Value::Float(1.0))]))]);
    let fields = flat(&snap);
    assert!(opentsdb::render_put(&fields[0], f64::NAN, 0).is_none());
    assert!(opentsdb::render_put(&fields[0], f64::INFINITY, 0).is_none());
    assert!(opentsdb::render_put(&fields[0], f64::NEG_INFINITY, 0).is_none());
}

#[test]
fn render_put_sanitizes_whitespace() {
    // OpenTSDB does not allow spaces in metric/tag names — the charset
    // is [a-zA-Z0-9_./-], everything else becomes '_'.
    let snap = obj(&[("cpu 0", obj(&[("rx bytes", Value::Float(1.0))]))]);
    let fields = flat(&snap);
    let line = opentsdb::render_put(&fields[0], 1.0, 100).unwrap();
    assert!(line.starts_with("put cpu_0.rx_bytes 100 1 plugin=cpu_0\n"), "got: {}", line);
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
    let body = opentsdb::build_body(&flat(&snap), 100);
    assert!(body.contains("put cpu.good 100 1 plugin=cpu\n"));
    assert!(!body.contains("bad"));
    // String values are skipped (not numeric).
    assert!(!body.contains("str"));
}

#[test]
fn array_plugin_elements_get_elem_tag() {
    let nic = obj(&[
        ("iface", Value::String("eth0".into())),
        ("rx", Value::Uint(5)),
    ]);
    let snap = obj(&[("network", Value::Array(vec![nic]))]);
    let mut keys = HashMap::new();
    keys.insert("network".to_string(), "iface");
    let body = opentsdb::build_body(&collect(&snap, &keys), 100);
    assert!(body.contains("put network.eth0.rx 100 5 plugin=network elem=eth0\n"),
        "got: {}", body);
}

#[test]
fn empty_snapshot_yields_empty_body() {
    let snap = Value::Object(BTreeMap::new());
    let body = opentsdb::build_body(&flat(&snap), 100);
    assert!(body.is_empty());
}

#[test]
fn default_port_is_opentsdb_standard() {
    assert_eq!(opentsdb::Config::default().port, 4242);
}
