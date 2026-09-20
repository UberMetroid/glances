//! Unit tests for the Cassandra CQL exporter (native protocol v4).

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::cassandra;
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
fn startup_frame_is_v4_with_cql_version_map() {
    // Frame header: version 0x04 | flags | stream | opcode | length.
    let f = cassandra::build_startup();
    assert_eq!(f[0], 0x04); // CQL v4 request
    assert_eq!(f[4], 0x01); // OPCODE_STARTUP
    let declared = i32::from_be_bytes([f[5], f[6], f[7], f[8]]) as usize;
    assert_eq!(declared, f.len() - 9);
    let raw = String::from_utf8_lossy(&f[9..]);
    assert!(raw.contains("CQL_VERSION"));
    assert!(raw.contains("3.0.0"));
}

#[test]
fn batch_frame_wraps_statements_with_consistency() {
    let stmts = vec!["INSERT INTO a.b (x) VALUES (1);".to_string()];
    let f = cassandra::build_batch(&stmts);
    assert_eq!(f[0], 0x04);
    assert_eq!(f[4], 0x0D); // OPCODE_BATCH
    let body = &f[9..];
    assert_eq!(body[0], 0); // unlogged batch
    assert_eq!(u16::from_be_bytes([body[1], body[2]]), 1); // 1 statement
    // Tail: consistency ONE (0x0001) + flags byte 0.
    assert_eq!(&body[body.len() - 3..], &[0, 1, 0]);
}

#[test]
fn nan_float_is_rendered_as_null_token() {
    let cfg = cassandra::Config::default();
    let snap = obj(&[("cpu", obj(&[("bad", Value::Float(f64::NAN))]))]);
    let stmts = cassandra::build_statements(&flat(&snap), &cfg);
    assert_eq!(stmts.len(), 1);
    // Non-finite floats are emitted as the literal NULL so cqlsh and
    // cassandra-stress can ingest the batch.
    assert!(stmts[0].ends_with("'NULL');"), "got: {}", stmts[0]);
}

#[test]
fn empty_keyspace_is_rejected() {
    let cfg = cassandra::Config { keyspace: String::new(), ..Default::default() };
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let err = cassandra::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}

#[test]
fn empty_snapshot_yields_no_statements() {
    let cfg = cassandra::Config::default();
    let snap = Value::Object(BTreeMap::new());
    assert!(cassandra::build_statements(&flat(&snap), &cfg).is_empty());
}

#[test]
fn keyspace_with_injection_chars_is_rejected() {
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let cfg = cassandra::Config { keyspace: "k; DROP".into(), ..Default::default() };
    let err = cassandra::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}
