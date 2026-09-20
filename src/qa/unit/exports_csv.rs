//! Unit tests for the CSV exporter (`src/exports/csv.rs`).

use std::collections::{BTreeMap, HashMap};
use std::fs;

use crate::core::value::Value;
use crate::exports::csv;
use crate::exports::flatten::{collect, Field};
use crate::qa::harness::TempDir;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
}

#[test]
fn overwrite_truncates_instead_of_appending() {
    let dir = TempDir::new("csv-overwrite");
    let path = dir.path().join("out.csv").to_string_lossy().to_string();
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(1.0))]))]);
    let append = csv::Config { path: path.clone(), timestamp: Some(1.0), ..Default::default() };
    csv::write(&flat(&snap), &append).expect("write");
    csv::write(&flat(&snap), &append).expect("write");
    let twice = fs::read_to_string(&path).unwrap();
    assert_eq!(twice.lines().count(), 3, "header + 2 rows: {}", twice);
    let trunc = csv::Config {
        path: path.clone(),
        timestamp: Some(1.0),
        overwrite: true,
        ..Default::default()
    };
    csv::write(&flat(&snap), &trunc).expect("write");
    let once = fs::read_to_string(&path).unwrap();
    assert_eq!(once.lines().count(), 2, "header + 1 row: {}", once);
}

#[test]
fn header_emitted_when_file_does_not_exist() {
    let dir = TempDir::new("csv-header");
    let path = dir.path().join("out.csv").to_string_lossy().to_string();
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(12.5))]))]);
    let cfg = csv::Config { path: path.clone(), timestamp: Some(1.0), ..Default::default() };
    csv::write(&flat(&snap), &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert!(body.starts_with("timestamp,plugin,key,value,unit,description\n"));
    assert!(body.contains("1,cpu,total,12.5,,\n"));
}

#[test]
fn header_skipped_when_file_already_exists() {
    let dir = TempDir::new("csv-append");
    let path = dir.path().join("out.csv").to_string_lossy().to_string();
    fs::write(&path, "preexisting,header\n").unwrap();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(7))]))]);
    let cfg = csv::Config { path: path.clone(), timestamp: Some(2.0), ..Default::default() };
    csv::write(&flat(&snap), &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert!(!body.contains("timestamp,plugin,key"));
    assert!(body.contains("preexisting,header\n2,cpu,total,7,,\n"));
}

#[test]
fn empty_path_is_rejected() {
    let cfg = csv::Config { path: String::new(), timestamp: Some(0.0), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(1))]))]);
    let err = csv::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn nan_value_renders_as_nan_string() {
    let dir = TempDir::new("csv-nan");
    let path = dir.path().join("out.csv").to_string_lossy().to_string();
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
    let cfg = csv::Config { path: path.clone(), timestamp: Some(0.0), ..Default::default() };
    csv::write(&flat(&snap), &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert!(body.contains("0,cpu,bad,NaN,,\n"));
}

#[test]
fn infinity_renders_with_sign() {
    let dir = TempDir::new("csv-inf");
    let path = dir.path().join("out.csv").to_string_lossy().to_string();
    let snap = obj(&[(
        "cpu",
        obj(&[("pos", Value::Float(f64::INFINITY)), ("neg", Value::Float(f64::NEG_INFINITY))]),
    )]);
    let cfg = csv::Config { path: path.clone(), timestamp: Some(0.0), ..Default::default() };
    csv::write(&flat(&snap), &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert!(body.contains("cpu,pos,Inf,,"));
    assert!(body.contains("cpu,neg,-Inf,,"));
}

#[test]
fn unicode_keys_and_values_pass_through() {
    let dir = TempDir::new("csv-unicode");
    let path = dir.path().join("out.csv").to_string_lossy().to_string();
    let snap = obj(&[("café", obj(&[("naïve", Value::String("héllo, wörld".into()))]))]);
    let cfg = csv::Config { path: path.clone(), timestamp: Some(0.0), ..Default::default() };
    csv::write(&flat(&snap), &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    // Comma in value triggers quoting (RFC 4180).
    assert!(body.contains("0,café,naïve,\"héllo, wörld\",,\n"));
}

#[test]
fn quotes_in_values_are_escaped() {
    let dir = TempDir::new("csv-quote");
    let path = dir.path().join("out.csv").to_string_lossy().to_string();
    let snap = obj(&[("cpu", obj(&[("msg", Value::String("he said \"hi\"".into()))]))]);
    let cfg = csv::Config { path: path.clone(), timestamp: Some(0.0), ..Default::default() };
    csv::write(&flat(&snap), &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert!(body.contains("cpu,msg,\"he said \"\"hi\"\"\",,\n"));
}

#[test]
fn array_plugin_elements_land_in_plugin_column() {
    let dir = TempDir::new("csv-array");
    let path = dir.path().join("out.csv").to_string_lossy().to_string();
    let nic = obj(&[
        ("iface", Value::String("eth0".into())),
        ("rx", Value::Uint(9)),
    ]);
    let snap = obj(&[("network", Value::Array(vec![nic]))]);
    let mut keys = HashMap::new();
    keys.insert("network".to_string(), "iface");
    let cfg = csv::Config { path: path.clone(), timestamp: Some(0.0), ..Default::default() };
    csv::write(&collect(&snap, &keys), &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    // The element identity joins the series: "network.eth0".
    assert!(body.contains("0,network.eth0,rx,9,,\n"), "got: {}", body);
}

#[test]
fn non_object_snapshot_writes_header_only() {
    let dir = TempDir::new("csv-shape");
    let path = dir.path().join("out.csv").to_string_lossy().to_string();
    let cfg = csv::Config { path: path.clone(), timestamp: Some(0.0), ..Default::default() };
    // A bare Array yields no flattened fields — write succeeds and
    // emits only the header row.
    let snap = Value::Array(vec![Value::Int(1)]);
    assert!(flat(&snap).is_empty());
    csv::write(&flat(&snap), &cfg).expect("write");
    let body = fs::read_to_string(&path).unwrap();
    assert_eq!(body, "timestamp,plugin,key,value,unit,description\n");
}
