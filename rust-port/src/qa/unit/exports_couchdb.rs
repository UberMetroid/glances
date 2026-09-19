//! Unit tests for the CouchDB JSON-document exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::couchdb;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn request_targets_database_collection_path() {
    let cfg = couchdb::Config::default();
    let raw = String::from_utf8(couchdb::build_request(&cfg, "{}")).unwrap();
    assert!(raw.starts_with("POST /glances/ HTTP/1.1\r\n"));
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
fn empty_database_rejected() {
    let cfg = couchdb::Config { database: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = couchdb::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn default_port_is_couchdb_standard() {
    let cfg = couchdb::Config::default();
    assert_eq!(cfg.port, 5984);
    assert_eq!(cfg.host, "127.0.0.1");
    assert_eq!(cfg.database, "glances");
}