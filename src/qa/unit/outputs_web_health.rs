//! Tests for the health rollup's alerts check: only recent entries
//! count, stale ones fade, and unknown age stays conservative.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::value::Value;
use crate::outputs::web::health::alert_check;

fn alert_entry(kind: &str, age_secs: f64) -> Value {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut o = BTreeMap::new();
    o.insert("type".to_string(), Value::String(kind.to_string()));
    o.insert("timestamp".to_string(), Value::Float(now - age_secs));
    Value::Object(o)
}

fn check(stats: &Value) -> (u8, String, String) {
    let mut out = Vec::new();
    alert_check(stats, &mut out);
    assert_eq!(out.len(), 1);
    out.pop().unwrap()
}

#[test]
fn stale_alerts_do_not_count() {
    let stats = Value::Array(vec![
        alert_entry("CRITICAL", 3600.0),
        alert_entry("WARNING", 120.0),
    ]);
    let (rank, name, detail) = check(&stats);
    assert_eq!((rank, name.as_str()), (0, "alerts"));
    assert_eq!(detail, "no active alerts");
}

#[test]
fn recent_alerts_count() {
    let stats = Value::Array(vec![
        alert_entry("CRITICAL", 5.0),
        alert_entry("CAREFUL", 10.0),
    ]);
    let (rank, _, detail) = check(&stats);
    assert_eq!(rank, 2);
    assert_eq!(detail, "1 critical · 1 warning");
}

#[test]
fn missing_timestamp_counts() {
    // Conservative: unknown age must never hide an alert.
    let mut o = BTreeMap::new();
    o.insert("type".to_string(), Value::String("WARNING".to_string()));
    let (rank, _, _) = check(&Value::Array(vec![Value::Object(o)]));
    assert_eq!(rank, 1);
}
