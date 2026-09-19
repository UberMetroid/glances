//! Unit tests for outputs::json_stdout (M12 JSON streaming).

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::outputs::json_stdout::{render_envelope, render_line};

fn obj(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

#[test]
fn envelope_has_timestamp_and_plugins_keys() {
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(12.5))]))]);
    let env = render_envelope(&snap, 1.5);
    let m = env.as_object().expect("envelope must be an object");
    assert!(m.contains_key("timestamp"));
    assert!(m.contains_key("plugins"));
    assert_eq!(m.get("timestamp"), Some(&Value::Float(1.5)));
}

#[test]
fn line_starts_with_timestamp_and_plugins() {
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(50.0))]))]);
    let line = render_line(&snap, 2.0);
    // render_line preserves insertion order (timestamp first).
    assert!(line.starts_with("{\"timestamp\":2.000000,\"plugins\":{"));
    assert!(line.contains("\"cpu\":{\"total\":50.000000}"));
}

#[test]
fn line_is_a_single_line() {
    let snap = obj(&[("mem", obj(&[("used", Value::Uint(2048))]))]);
    let line = render_line(&snap, 0.0);
    assert!(!line.contains('\n'), "JSON line should not contain raw newlines");
}

#[test]
fn nan_floats_become_null_in_output() {
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
    let line = render_line(&snap, 0.0);
    assert!(line.contains("\"bad\":null"));
}

#[test]
fn infinity_floats_become_null_in_output() {
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::INFINITY))]))]);
    let line = render_line(&snap, 0.0);
    assert!(line.contains("\"bad\":null"));
}

#[test]
fn empty_snapshot_yields_empty_plugins_object() {
    let snap = Value::Object(BTreeMap::new());
    let line = render_line(&snap, 1.0);
    // render_line preserves insertion order (timestamp first).
    assert_eq!(line, "{\"timestamp\":1.000000,\"plugins\":{}}");
}

#[test]
fn arrays_kept_as_json_arrays() {
    let arr = Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let inner = obj(&[("cores", arr)]);
    let snap = obj(&[("cpu", inner)]);
    let line = render_line(&snap, 0.0);
    assert!(line.contains("\"cores\":[1,2,3]"));
}
