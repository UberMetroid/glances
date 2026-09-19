//! Unit tests for core::value (Value enum + JSON serializer).

use crate::core::value::{to_json, Value};
use std::collections::BTreeMap;

#[test]
fn null_serializes() { assert_eq!(to_json(&Value::Null), "null"); }

#[test]
fn bool_serializes() {
    assert_eq!(to_json(&Value::Bool(true)), "true");
    assert_eq!(to_json(&Value::Bool(false)), "false");
}

#[test]
fn int_uint_float_serialize() {
    assert_eq!(to_json(&Value::Int(-42)), "-42");
    assert_eq!(to_json(&Value::Uint(42)), "42");
    assert_eq!(to_json(&Value::Float(3.5)), "3.5");
}

#[test]
fn nan_becomes_null() {
    // Per plan: no NaN/Infinity in JSON output (matches Python json_dumps).
    assert_eq!(to_json(&Value::Float(f64::NAN)), "null");
    assert_eq!(to_json(&Value::Float(f64::INFINITY)), "null");
}

#[test]
fn string_escapes_special_chars() {
    let s = Value::String("a\"b\\c\nd".into());
    assert_eq!(to_json(&s), "\"a\\\"b\\\\c\\nd\"");
}

#[test]
fn array_object_round_trip() {
    let mut obj = BTreeMap::new();
    obj.insert("k".into(), Value::Int(1));
    let v = Value::Array(vec![Value::Int(1), Value::String("two".into()), Value::Object(obj)]);
    assert_eq!(to_json(&v), "[1,\"two\",{\"k\":1}]");
}

#[test]
fn accessors() {
    let mut obj = BTreeMap::new();
    obj.insert("k".into(), Value::Int(1));
    let v = Value::Object(obj.clone());
    assert!(v.is_object());
    assert_eq!(v.as_object().unwrap().get("k"), Some(&Value::Int(1)));
    let arr = Value::Array(vec![Value::Int(1)]);
    assert!(arr.is_array());
    assert_eq!(arr.as_array().unwrap().len(), 1);
}

#[test]
fn as_f64_i64_conversions() {
    assert_eq!(Value::Int(7).as_f64(), Some(7.0));
    assert_eq!(Value::Uint(8).as_i64(), Some(8));
    assert_eq!(Value::Float(2.5).as_i64(), Some(2));
    assert_eq!(Value::String("x".into()).as_f64(), None);
}
