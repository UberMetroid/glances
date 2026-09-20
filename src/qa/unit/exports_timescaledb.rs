//! Unit tests for the TimescaleDB exporter (pure builders + MD5 vectors;
//! no live server required).

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::timescaledb;

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
fn md5_matches_rfc1321_vectors() {
    assert_eq!(timescaledb::md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(timescaledb::md5_hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
    assert_eq!(timescaledb::md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(
        timescaledb::md5_hex(b"message digest"),
        "f96b697d7cb7938d525a2f31aaf161d0"
    );
}

#[test]
fn quote_ident_doubles_quotes() {
    assert_eq!(timescaledb::quote_ident("plain"), "\"plain\"");
    assert_eq!(timescaledb::quote_ident("a\"b"), "\"a\"\"b\"");
}

#[test]
fn sql_literal_formats_values() {
    assert_eq!(timescaledb::sql_literal(&Value::Bool(true)), "TRUE");
    assert_eq!(timescaledb::sql_literal(&Value::Int(-3)), "-3");
    assert_eq!(timescaledb::sql_literal(&Value::Uint(7)), "7");
    assert_eq!(timescaledb::sql_literal(&Value::Float(f64::NAN)), "NULL");
    assert_eq!(timescaledb::sql_literal(&Value::String("o'x".into())), "'o''x'");
    assert_eq!(timescaledb::sql_literal(&Value::Null), "NULL");
}

#[test]
fn build_tables_groups_dict_plugin() {
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(42.0))]))]);
    let tables = timescaledb::build_tables(&flat(&snap));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].name, "cpu");
    assert!(!tables[0].has_key);
    let ddl = timescaledb::create_table_sql(&tables[0]);
    assert!(ddl.contains("CREATE TABLE IF NOT EXISTS \"cpu\""), "got: {}", ddl);
    assert!(ddl.contains("\"total\" DOUBLE PRECISION NULL"), "got: {}", ddl);
    let insert = timescaledb::insert_sql(&tables[0], "host1", 1_700_000_000).unwrap();
    assert!(insert.contains("to_timestamp(1700000000)"), "got: {}", insert);
    assert!(insert.contains("'host1'"), "got: {}", insert);
}

#[test]
fn build_tables_adds_key_column_for_elements() {
    // Two elements with disjoint keys exercise the column union + NULL fill.
    let mut keys = HashMap::new();
    keys.insert("network".to_string(), "alias");
    let snap = obj(&[(
        "network",
        Value::Array(vec![
            obj(&[("alias", Value::String("eth0".into())), ("rx", Value::Uint(1))]),
            obj(&[("alias", Value::String("wlan0".into())), ("tx", Value::Uint(2))]),
        ]),
    )]);
    let tables = timescaledb::build_tables(&collect(&snap, &keys));
    assert_eq!(tables.len(), 1);
    assert!(tables[0].has_key);
    assert_eq!(tables[0].rows.len(), 2);
    let insert = timescaledb::insert_sql(&tables[0], "h", 100).unwrap();
    assert!(insert.contains("NULL"), "union fill must emit NULL: {}", insert);
    assert!(insert.contains("'eth0'") && insert.contains("'wlan0'"), "got: {}", insert);
}

#[test]
fn effective_user_prefers_config_then_env() {
    let mut c = timescaledb::Config::default();
    c.user = "alice".into();
    assert_eq!(timescaledb::effective_user(&c), "alice");
    c.user.clear();
    let u = timescaledb::effective_user(&c);
    assert!(!u.is_empty());
}
