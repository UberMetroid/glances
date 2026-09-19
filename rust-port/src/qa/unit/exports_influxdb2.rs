//! Unit tests for the InfluxDB v2 HTTP exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::influxdb2;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn request_targets_v2_write_endpoint_with_query_params() {
    let cfg = influxdb2::Config::default();
    let raw = String::from_utf8(influxdb2::build_request(&cfg, "x")).unwrap();
    assert!(raw.starts_with("POST /api/v2/write?org=glances&bucket=glances HTTP/1.1\r\n"));
    assert!(raw.contains("Content-Type: text/plain"));
}

#[test]
fn token_emits_authorization_header() {
    let cfg = influxdb2::Config { token: "abc123".into(), ..Default::default() };
    let raw = String::from_utf8(influxdb2::build_request(&cfg, "x")).unwrap();
    assert!(raw.contains("Authorization: Token abc123\r\n"));
}

#[test]
fn no_token_omits_authorization_header() {
    let cfg = influxdb2::Config { token: String::new(), ..Default::default() };
    let raw = String::from_utf8(influxdb2::build_request(&cfg, "x")).unwrap();
    assert!(!raw.contains("Authorization"));
}

#[test]
fn body_renders_line_protocol_with_timestamps() {
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(42))]))]);
    let body = influxdb2::build_body(&snap, Some(1.0));
    assert!(body.starts_with("cpu total=42i "));
    // 1.0 s = 1e9 ns.
    assert!(body.contains(" 1000000000\n"));
}

#[test]
fn empty_org_or_bucket_rejected() {
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let cfg = influxdb2::Config { org: String::new(), ..Default::default() };
    let err = influxdb2::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
    let cfg = influxdb2::Config { bucket: String::new(), ..Default::default() };
    let err = influxdb2::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}