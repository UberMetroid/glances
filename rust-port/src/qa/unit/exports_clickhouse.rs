//! Unit tests for the ClickHouse HTTP exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::clickhouse;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn request_targets_insert_with_json_each_row_format() {
    let cfg = clickhouse::Config::default();
    let req = clickhouse::build_request(&cfg, "{\"plugin\":\"cpu\"}\n");
    let raw = String::from_utf8(req).unwrap();
    assert!(raw.starts_with("POST /?query=INSERT+INTO+default.glances_stats+FORMAT+JSONEachRow HTTP/1.1\r\n"));
    assert!(raw.contains("Content-Type: application/x-ndjson"));
    assert!(raw.ends_with("{\"plugin\":\"cpu\"}\n"));
}

#[test]
fn content_length_header_matches_body() {
    let cfg = clickhouse::Config::default();
    let body = "{\"x\":1}\n";
    let raw = String::from_utf8(clickhouse::build_request(&cfg, body)).unwrap();
    let head = raw.split("\r\n\r\n").next().unwrap();
    let declared = head
        .lines()
        .find(|l| l.starts_with("Content-Length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap();
    assert_eq!(declared, body.len());
}

#[test]
fn nan_floats_are_dropped_from_json_each_row_body() {
    let snap = obj(&[(
        "cpu",
        obj(&[("bad", Value::Float(f64::NAN)), ("ok", Value::Int(1))]),
    )]);
    let body = clickhouse::build_body(&snap);
    assert!(!body.contains("\"bad\""));
    // Each row is rendered as {"plugin":..., "key":"ok", "value":1}.
    assert!(body.contains("\"key\":\"ok\""));
    assert!(body.contains("\"value\":1"));
}

#[test]
fn host_with_port_in_query_string() {
    let cfg = clickhouse::Config { database: "t".into(), table: "u".into(), ..Default::default() };
    let raw = String::from_utf8(clickhouse::build_request(&cfg, "")).unwrap();
    // Database and table names appear URL-encoded with + (space) — verify.
    assert!(raw.contains("INSERT+INTO+t.u+FORMAT+JSONEachRow"));
}

#[test]
fn empty_table_rejected() {
    let cfg = clickhouse::Config { table: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = clickhouse::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}