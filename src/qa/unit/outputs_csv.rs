//! Unit tests for outputs::csv_stdout (M12 CSV streaming).

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::outputs::csv_stdout::{
    csv_escape, render_rows, value_to_cell, HEADER,
};

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

#[test]
fn header_has_six_columns() {
    let parts: Vec<&str> = HEADER.split(',').collect();
    assert_eq!(parts.len(), 6);
    assert_eq!(parts[0], "timestamp");
    assert_eq!(parts[5], "description");
}

#[test]
fn csv_escape_no_special_chars() {
    assert_eq!(csv_escape("plain"), "plain");
    assert_eq!(csv_escape("eth0"), "eth0");
    assert_eq!(csv_escape(""), "");
}

#[test]
fn csv_escape_quotes_and_commas() {
    assert_eq!(csv_escape("a,b"), "\"a,b\"");
    assert_eq!(csv_escape("he said \"hi\""), "\"he said \"\"hi\"\"\"");
}

#[test]
fn csv_escape_newlines_trigger_quoting() {
    assert_eq!(csv_escape("a\nb"), "\"a\nb\"");
    assert_eq!(csv_escape("a\rb"), "\"a\rb\"");
}

// 3.14159/-2.71828 are arbitrary rounding fixtures, not PI/E.
#[allow(clippy::approx_constant)]
#[test]
fn float_cell_uses_two_decimals() {
    assert_eq!(value_to_cell(&Value::Float(3.14159)), "3.14");
    assert_eq!(value_to_cell(&Value::Float(0.0)), "0.00");
    assert_eq!(value_to_cell(&Value::Float(-2.71828)), "-2.72");
}

#[test]
fn float_cell_handles_nan_and_infinity() {
    assert_eq!(value_to_cell(&Value::Float(f64::NAN)), "NaN");
    assert_eq!(value_to_cell(&Value::Float(f64::INFINITY)), "Inf");
    assert_eq!(value_to_cell(&Value::Float(f64::NEG_INFINITY)), "-Inf");
}

#[test]
fn integer_and_bool_cells() {
    assert_eq!(value_to_cell(&Value::Int(-42)), "-42");
    assert_eq!(value_to_cell(&Value::Uint(7)), "7");
    assert_eq!(value_to_cell(&Value::Bool(true)), "true");
    assert_eq!(value_to_cell(&Value::Bool(false)), "false");
    assert_eq!(value_to_cell(&Value::Null), "");
}

#[test]
fn render_object_emits_one_row_per_key() {
    let cpu = obj(&[
        ("total", Value::Float(50.0)),
        ("user", Value::Float(20.0)),
    ]);
    let snap = obj(&[("cpu", cpu)]);
    let rows = render_rows(&snap, 1.0);
    assert_eq!(rows.len(), 2);
    let joined = rows.join("\n");
    assert!(joined.contains("1.000,cpu,total,50.00,,"));
    assert!(joined.contains("1.000,cpu,user,20.00,,"));
}

#[test]
fn render_array_uses_key_field_as_ident() {
    let mut e1 = BTreeMap::new();
    e1.insert("key".into(), Value::String("eth0".into()));
    e1.insert("rx".into(), Value::Float(100.0));
    let net = Value::Array(vec![Value::Object(e1)]);
    let snap = obj(&[("network", net)]);
    let rows = render_rows(&snap, 2.5);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains(",network,"));
    assert!(rows[0].contains("eth0"));
}

#[test]
fn render_empty_snapshot_emits_no_rows() {
    let snap = Value::Object(BTreeMap::new());
    let rows = render_rows(&snap, 0.0);
    assert!(rows.is_empty());
}

#[test]
fn render_skips_plugin_with_null_stats() {
    let snap = obj(&[("nowhere", Value::Null)]);
    let rows = render_rows(&snap, 1.0);
    assert_eq!(rows.len(), 1);
}

#[test]
fn filter_plugins_keeps_listed_only() {
    use crate::outputs::filter::filter_plugins;
    let snap = obj(&[
        ("cpu", Value::Float(1.0)),
        ("mem", Value::Float(2.0)),
    ]);
    assert_eq!(filter_plugins(&snap, &None), snap);
    assert_eq!(filter_plugins(&snap, &Some("".into())), snap);
    let keep = filter_plugins(&snap, &Some("cpu".into()));
    let o = keep.as_object().expect("object");
    assert!(o.contains_key("cpu") && !o.contains_key("mem"));
}
