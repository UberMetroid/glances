//! Unit tests for the Cassandra CQL exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::cassandra;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn insert_targets_keyspace_and_table() {
    let cfg = cassandra::Config::default();
    let sql = cassandra::render_insert(&cfg, "cpu", "total", "42");
    assert_eq!(
        sql,
        "INSERT INTO glances.stats (plugin, key, value) VALUES ('cpu', 'total', '42');"
    );
}

#[test]
fn apostrophes_in_values_are_doubled() {
    let cfg = cassandra::Config::default();
    let sql = cassandra::render_insert(&cfg, "cpu", "name", "it's ok");
    // Single quote inside a CQL string literal must be escaped as ''.
    assert!(sql.contains("'it''s ok'"));
}

#[test]
fn nan_float_is_rendered_as_null_token() {
    let cfg = cassandra::Config::default();
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
    let body = cassandra::build_body(&snap, &cfg);
    // The exporter emits the literal "NULL" for non-finite floats so
    // cassandra-stress / cqlsh parsers can ingest it.
    assert!(body.contains("'NULL'"));
}

#[test]
fn empty_keyspace_is_rejected() {
    let cfg = cassandra::Config { keyspace: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = cassandra::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn empty_snapshot_yields_empty_body() {
    let cfg = cassandra::Config::default();
    let snap = Value::Object(BTreeMap::new());
    assert!(cassandra::build_body(&snap, &cfg).is_empty());
}