//! Unit tests for the DuckDB exporter (pure SQL builders; no live
//! DuckDB required except the binary-absent error path).

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::duckdb;
use crate::exports::flatten::{collect, Field};

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
}

#[test]
fn quote_ident_doubles_quotes() {
    assert_eq!(duckdb::quote_ident("plain"), "\"plain\"");
    assert_eq!(duckdb::quote_ident("a\"b"), "\"a\"\"b\"");
}

#[test]
fn sql_literal_formats_values() {
    assert_eq!(duckdb::sql_literal(&Value::Bool(false)), "FALSE");
    assert_eq!(duckdb::sql_literal(&Value::Int(-3)), "-3");
    assert_eq!(duckdb::sql_literal(&Value::Float(f64::INFINITY)), "NULL");
    assert_eq!(duckdb::sql_literal(&Value::String("o'x".into())), "'o''x'");
    assert_eq!(duckdb::sql_literal(&Value::Null), "NULL");
}

#[test]
fn build_scripts_emit_create_and_insert() {
    let snap = obj(&[(
        "cpu",
        obj(&[("total", Value::Float(42.0)), ("name", Value::String("x".into()))]),
    )]);
    let scripts = duckdb::build_scripts(&flat(&snap), "host1", 1_700_000_000);
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].table, "cpu");
    assert!(
        scripts[0].create.contains("CREATE TABLE IF NOT EXISTS \"cpu\""),
        "got: {}",
        scripts[0].create
    );
    assert!(scripts[0].create.contains("\"total\" DOUBLE"), "got: {}", scripts[0].create);
    let insert = scripts[0].insert.as_ref().expect("insert");
    assert!(insert.contains("to_timestamp(1700000000)"), "got: {}", insert);
    assert!(insert.contains("'host1'"), "got: {}", insert);
    assert!(insert.contains("\"time\", \"hostname\""), "got: {}", insert);
}

#[test]
fn build_scripts_add_key_column_for_elements() {
    let mut keys = HashMap::new();
    keys.insert("fs".to_string(), "mntpoint");
    let snap = obj(&[(
        "fs",
        Value::Array(vec![obj(&[
            ("mntpoint", Value::String("/".into())),
            ("percent", Value::Float(10.0)),
        ])]),
    )]);
    let scripts = duckdb::build_scripts(&collect(&snap, &keys), "h", 100);
    assert_eq!(scripts.len(), 1);
    assert!(scripts[0].create.contains("\"key\" VARCHAR"), "got: {}", scripts[0].create);
    let insert = scripts[0].insert.as_ref().expect("insert");
    assert!(insert.contains("'/'"), "got: {}", insert);
}

#[test]
fn write_errors_clearly_without_binary() {
    if duckdb::duckdb_bin().is_some() {
        return;
    }
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(1.0))]))]);
    let err = duckdb::write(&flat(&snap), &duckdb::Config::default()).unwrap_err();
    assert!(err.to_string().contains("duckdb"), "got: {}", err);
}
