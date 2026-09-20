//! Unit tests for the CouchDB JSON-document exporter.

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::couchdb;
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
fn request_targets_bulk_docs_endpoint() {
    let cfg = couchdb::Config::default();
    let raw = String::from_utf8(couchdb::build_request(&cfg, "{}")).unwrap();
    // One _bulk_docs call per tick — posting concatenated docs to the
    // database root is rejected by CouchDB as a malformed body.
    assert!(raw.starts_with("POST /glances/_bulk_docs HTTP/1.1\r\n"), "got: {}", raw);
    assert!(raw.contains("Host: 127.0.0.1:5984"));
    assert!(raw.contains("Content-Type: application/json"));
    // No auth header when none configured.
    assert!(!raw.contains("Authorization"));
}

#[test]
fn basic_auth_header_emitted_when_user_and_pass_set() {
    let cfg = couchdb::Config {
        auth_user: Some("alice".into()),
        auth_pass: Some("secret".into()),
        ..Default::default()
    };
    let raw = String::from_utf8(couchdb::build_request(&cfg, "{}")).unwrap();
    // base64("alice:secret") = "YWxpY2U6c2VjcmV0"
    assert!(raw.contains("Authorization: Basic YWxpY2U6c2VjcmV0"));
}

#[test]
fn content_length_header_matches_body() {
    let cfg = couchdb::Config::default();
    let body = "{\"plugin\":\"cpu\"}";
    let raw = String::from_utf8(couchdb::build_request(&cfg, body)).unwrap();
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
fn body_is_single_bulk_docs_envelope() {
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let body = couchdb::build_body(&flat(&snap));
    assert!(body.starts_with("{\"docs\":[{"), "got: {}", body);
    assert!(body.ends_with("]}"), "got: {}", body);
    assert!(body.contains("\"plugin\":\"cpu\""));
    assert!(body.contains("\"key\":\"x\""));
    assert!(body.contains("\"value\":1"));
    // One JSON object on a single line — no NDJSON framing here.
    assert!(!body.contains('\n'));
}

#[test]
fn nan_floats_are_dropped_from_docs() {
    let snap = obj(&[(
        "cpu",
        obj(&[("bad", Value::Float(f64::NAN)), ("ok", Value::Int(1))]),
    )]);
    let body = couchdb::build_body(&flat(&snap));
    assert!(!body.contains("bad"), "NaN doc leaked: {}", body);
    assert!(body.contains("\"ok\""));
}

#[test]
fn empty_database_rejected() {
    let cfg = couchdb::Config { database: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = couchdb::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn default_port_is_couchdb_standard() {
    let cfg = couchdb::Config::default();
    assert_eq!(cfg.port, 5984);
    assert_eq!(cfg.host, "127.0.0.1");
    assert_eq!(cfg.database, "glances");
}
