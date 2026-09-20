//! Unit tests for the Elasticsearch _bulk exporter.

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::elasticsearch;
use crate::exports::flatten::{collect, Field};

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
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
    let body = elasticsearch::build_body(&flat(&snap), &cfg.index, 1000);
    let lines: Vec<&str> = body.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"_index\":\"glances\""));
    // Time-series insert — no _id (a stable id would overwrite the
    // previous tick's document).
    assert!(!lines[0].contains("\"_id\""));
    assert!(lines[1].contains("\"plugin\":\"cpu\""));
    assert!(lines[1].contains("\"key\":\"total\""));
    assert!(lines[1].contains("\"ts_ms\":1000"));
    assert!(lines[1].contains("\"value\":42"));
    // ES requires a trailing newline after the last document.
    assert!(body.ends_with("}\n"));
}

#[test]
fn array_plugin_docs_carry_elem_field() {
    let nic = obj(&[
        ("iface", Value::String("eth0".into())),
        ("rx", Value::Uint(9)),
    ]);
    let snap = obj(&[("network", Value::Array(vec![nic]))]);
    let mut keys = HashMap::new();
    keys.insert("network".to_string(), "iface");
    let body = elasticsearch::build_body(&collect(&snap, &keys), "glances", 0);
    assert!(body.contains("\"series\":\"network.eth0\""), "got: {}", body);
    assert!(body.contains("\"elem\":\"eth0\""), "got: {}", body);
}

#[test]
fn nan_renders_as_null_in_doc_field() {
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
    let body = elasticsearch::build_body(&flat(&snap), "glances", 0);
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
    let err = elasticsearch::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}
