//! Unit tests for the JSON exporter (`src/exports/json.rs`).

use std::collections::BTreeMap;
use std::fs;

use crate::core::value::Value;
use crate::exports::json;
use crate::qa::harness::TempDir;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

#[test]
fn writes_one_json_line_per_call() {
    let dir = TempDir::new("json-line");
    let path = dir.path().join("out.jsonl").to_string_lossy().to_string();
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(12.5))]))]);
    let cfg = json::Config { path: path.clone(), timestamp: Some(100.0) };
    json::write(&snap, &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert_eq!(body.lines().count(), 1);
    assert!(body.starts_with("{\"timestamp\":100"));
    assert!(body.contains("\"stats\""));
    assert!(body.contains("\"cpu\""));
}

#[test]
fn appends_to_existing_file() {
    let dir = TempDir::new("json-append");
    let path = dir.path().join("out.jsonl").to_string_lossy().to_string();
    let cfg = json::Config { path: path.clone(), timestamp: Some(1.0) };
    let snap1 = obj(&[("cpu", obj(&[("total", Value::Int(10))]))]);
    let snap2 = obj(&[("cpu", obj(&[("total", Value::Int(20))]))]);
    json::write(&snap1, &cfg).expect("write 1");
    json::write(&snap2, &cfg).expect("write 2");
    let body = fs::read_to_string(&path).unwrap();
    assert_eq!(body.lines().count(), 2);
    assert!(body.contains("\"total\":10"));
    assert!(body.contains("\"total\":20"));
}

#[test]
fn nan_in_stats_renders_as_null() {
    let dir = TempDir::new("json-nan");
    let path = dir.path().join("out.jsonl").to_string_lossy().to_string();
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
    let cfg = json::Config { path: path.clone(), timestamp: Some(0.0) };
    json::write(&snap, &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert!(body.contains("\"bad\":null"));
}

#[test]
fn special_chars_in_strings_are_escaped() {
    let dir = TempDir::new("json-special");
    let path = dir.path().join("out.jsonl").to_string_lossy().to_string();
    let snap = obj(&[(
        "cpu",
        obj(&[("msg", Value::String("a\"b\\c\nd".into()))]),
    )]);
    let cfg = json::Config { path: path.clone(), timestamp: Some(0.0) };
    json::write(&snap, &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    // to_json escapes: " → \", \ → \\, \n → \n (literal backslash-n)
    assert!(body.contains("\\\"a\\\"b\\\\c\\nd\\\""));
}

#[test]
fn empty_path_is_rejected() {
    let cfg = json::Config { path: String::new(), timestamp: Some(0.0) };
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(1))]))]);
    let err = json::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn empty_snapshot_yields_valid_envelope() {
    let dir = TempDir::new("json-empty");
    let path = dir.path().join("out.jsonl").to_string_lossy().to_string();
    let snap = Value::Object(BTreeMap::new());
    let cfg = json::Config { path: path.clone(), timestamp: Some(7.0) };
    json::write(&snap, &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    let line = body.trim_end_matches('\n');
    assert!(line.starts_with("{\"timestamp\":7"));
    assert!(line.contains("\"stats\":{}"));
}