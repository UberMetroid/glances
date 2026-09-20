//! SQL builders for the TimescaleDB exporter: table grouping,
//! DDL/INSERT generation, and identifier/literal quoting (all pure).

use std::collections::BTreeMap;

use crate::core::value::{self, Value};
use crate::exports::flatten::Field;

pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Render a value as a SQL literal. Non-finite floats and unsupported
/// shapes become NULL (never NaN text, which PostgreSQL rejects).
pub fn sql_literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        Value::Int(i) => format!("{}", i),
        Value::Uint(u) => format!("{}", u),
        Value::Float(f) if f.is_nan() || f.is_infinite() => "NULL".to_string(),
        Value::Float(f) => format!("{}", f),
        Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        Value::Object(_) | Value::Array(_) => {
            format!("'{}'", value::to_json(v).replace('\'', "''"))
        }
    }
}

/// Map a value to its column type (upstream `convert_types` parity).
pub fn sql_type(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "BOOLEAN",
        Value::Int(_) | Value::Uint(_) => "BIGINT",
        Value::Float(_) => "DOUBLE PRECISION",
        _ => "TEXT",
    }
}

/// One plugin table: ordered columns plus literal rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub name: String,
    pub has_key: bool,
    pub columns: Vec<(String, &'static str)>,
    pub rows: Vec<Vec<(Option<String>, Value)>>,
}

/// Group flattened fields by plugin into tables. Column union spans all
/// rows (missing cells become NULL); element series gain a `key_id`
/// column holding the element name.
pub fn build_tables(fields: &[Field<'_>]) -> Vec<Table> {
    let mut order: Vec<&str> = Vec::new();
    let mut groups: BTreeMap<&str, Vec<&Field<'_>>> = BTreeMap::new();
    for f in fields {
        if !order.contains(&f.plugin) {
            order.push(f.plugin);
        }
        groups.entry(f.plugin).or_default().push(f);
    }
    let mut tables = Vec::new();
    for plugin in order {
        let group = &groups[plugin];
        let has_key = group.iter().any(|f| f.elem.is_some());
        // Union of (elem, key) cells in first-seen order.
        let mut cols: Vec<(Option<String>, String, &'static str)> = Vec::new();
        for f in group.iter() {
            if !cols.iter().any(|(e, k, _)| *e == f.elem && *k == f.key) {
                cols.push((f.elem.clone(), f.key.to_string(), sql_type(f.value)));
            }
        }
        // One row per element (or a single row for dict plugins).
        let mut elem_order: Vec<Option<String>> = Vec::new();
        for f in group.iter() {
            if !elem_order.contains(&f.elem) {
                elem_order.push(f.elem.clone());
            }
        }
        let mut rows = Vec::new();
        for elem in &elem_order {
            let mut row = Vec::new();
            for (_, ck, _) in &cols {
                let v = group
                    .iter()
                    .find(|f| &f.elem == elem && f.key == ck)
                    .map(|f| (*f.value).clone())
                    .unwrap_or(Value::Null);
                row.push((elem.clone(), v));
            }
            rows.push(row);
        }
        tables.push(Table {
            name: plugin.to_string(),
            has_key,
            columns: cols.into_iter().map(|(_, k, t)| (k, t)).collect(),
            rows,
        });
    }
    tables
}

/// `CREATE TABLE IF NOT EXISTS` for one plugin table.
pub fn create_table_sql(t: &Table) -> String {
    let mut cols = vec![
        "\"time\" TIMESTAMPTZ NOT NULL".to_string(),
        "\"hostname_id\" TEXT NOT NULL".to_string(),
    ];
    if t.has_key {
        cols.push("\"key_id\" TEXT NOT NULL".to_string());
    }
    for (name, typ) in &t.columns {
        cols.push(format!("{} {} NULL", quote_ident(name), typ));
    }
    format!("CREATE TABLE IF NOT EXISTS {} ({})", quote_ident(&t.name), cols.join(", "))
}

/// Multi-row `INSERT` for one plugin table (`to_timestamp` keeps time
/// formatting server-side so no datetime code is needed).
pub fn insert_sql(t: &Table, hostname: &str, ts: i64) -> Option<String> {
    if t.rows.is_empty() {
        return None;
    }
    let mut header = vec!["\"time\"".to_string(), "\"hostname_id\"".to_string()];
    if t.has_key {
        header.push("\"key_id\"".to_string());
    }
    for (name, _) in &t.columns {
        header.push(quote_ident(name));
    }
    let mut rows = Vec::new();
    for row in &t.rows {
        let mut cells = vec![format!("to_timestamp({})", ts), format!("'{}'", hostname.replace('\'', "''"))];
        if t.has_key {
            let key = row
                .first()
                .and_then(|(e, _)| e.clone())
                .unwrap_or_default();
            cells.push(format!("'{}'", key.replace('\'', "''")));
        }
        for (_, v) in row {
            cells.push(sql_literal(v));
        }
        rows.push(format!("({})", cells.join(", ")));
    }
    Some(format!(
        "INSERT INTO {} ({}) VALUES {}",
        quote_ident(&t.name),
        header.join(", "),
        rows.join(", ")
    ))
}
