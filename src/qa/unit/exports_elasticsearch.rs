//! Unit tests for the Elasticsearch _bulk exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::elasticsearch;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn request_targets_bulk_endpoint() {
    let cfg = elasticsearch::Config::default();
    let raw = String::from_utf8(elasticsearch::build_request(&cfg, "")).unwrap();
    assert!(raw.starts_with("POST /_bulk HTTP/1.1\r\n"));
    assert!(raw.contains("Content-Type: application/x-ndjson"));
    assert!(!raw.contains("Authorization"));
}

#[test]
fn body_is_pairs_of_action_and_doc_with_trailing_newline() {
    let cfg = elasticsearch::Config::default();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(42))]))]);
    let body = elasticsearch::build_body(&snap, &cfg.index);
    // ES requires final newline. Strip empty lines and count.
    let lines: Vec<&str> = body.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"_index\":\"glances\""));
    assert!(lines[0].contains("\"_id\":\"cpu.total\""));
    assert!(lines[1].contains("\"plugin\":\"cpu\""));
    assert!(lines[1].contains("\"value\":42"));
    // Final newline is present (the byte after the last \n).
    assert!(body.ends_with("\n\n"));
}

#[test]
fn nan_renders_as_null_in_doc_field() {
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
    let body = elasticsearch::build_body(&snap, "glances");
    // ES strict_json mapping does not accept NaN; we render as null.
    assert!(body.contains("\"value\":null"));
}

#[test]
fn auth_emits_basic_authorization_header() {
    let cfg = elasticsearch::Config {
        auth_user: Some("u".into()),
        auth_pass: Some("p".into()),
        ..Default::default()
    };
    let raw = String::from_utf8(elasticsearch::build_request(&cfg, "")).unwrap();
    // base64("u:p") = "dTpw"
    assert!(raw.contains("Authorization: Basic dTpw"));
}

#[test]
fn empty_index_rejected() {
    let cfg = elasticsearch::Config { index: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = elasticsearch::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}