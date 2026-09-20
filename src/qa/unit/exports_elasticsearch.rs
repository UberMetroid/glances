//! Unit tests for the Elasticsearch _bulk exporter (upstream shape parity).

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
fn body_is_one_doc_per_plugin_with_dated_index() {
    let cfg = elasticsearch::Config::default();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(42))]))]);
    let body = elasticsearch::build_body(&snap, &cfg.index, "2024.01.02", "2024-01-02T03:04:05");
    let lines: Vec<&str> = body.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"_index\":\"glances-2024.01.02\""), "got: {}", lines[0]);
    assert!(lines[0].contains("\"_id\":\"cpu.2024-01-02T03:04:05\""), "got: {}", lines[0]);
    assert!(lines[0].contains("\"_type\":\"glances-cpu\""), "got: {}", lines[0]);
    assert!(lines[1].contains("\"plugin\":\"cpu\""), "got: {}", lines[1]);
    assert!(lines[1].contains("\"timestamp\":\"2024-01-02T03:04:05\""), "got: {}", lines[1]);
    assert!(lines[1].contains("\"total\":\"42\""), "got: {}", lines[1]);
    // ES requires a trailing newline after the last document.
    assert!(body.ends_with("}\n"));
}

#[test]
fn array_plugin_columns_use_index_paths() {
    let nic = obj(&[
        ("interface_name", Value::String("eth0".into())),
        ("bytes_recv", Value::Uint(9)),
    ]);
    let snap = obj(&[("network", Value::Array(vec![nic]))]);
    let body = elasticsearch::build_body(&snap, "glances", "2024.01.02", "2024-01-02T03:04:05");
    assert!(body.contains("\"0.interface_name\":\"eth0\""), "got: {}", body);
    assert!(body.contains("\"0.bytes_recv\":\"9\""), "got: {}", body);
}

#[test]
fn nan_renders_as_null_in_doc_field() {
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
    let body = elasticsearch::build_body(&snap, "glances", "2024.01.02", "2024-01-02T03:04:05");
    // ES strict_json mapping does not accept NaN; we render as null.
    assert!(body.contains("\"bad\":\"null\""), "got: {}", body);
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
