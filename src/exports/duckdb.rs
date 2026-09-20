//! DuckDB exporter — writes one table per plugin via the `duckdb` CLI.
//!
//! Mirrors `glances/exports/glances_duckdb/__init__.py` (embedded
//! `duckdb` module): tables carry `time`, `hostname`, an optional `key`
//! for element series, and one typed column per field
//! (bool→BOOLEAN, int→BIGINT, float→DOUBLE, else VARCHAR).
//!
//! Zero-dependency constraint: DuckDB is an embedded engine with no wire
//! protocol, so the port drives the external `duckdb` binary argv-only
//! (same pattern as `smartctl`/`virsh`) instead of linking the engine.
//! SQL is passed on stdin — never the shell. Missing binary or a failed
//! run returns an error (upstream exits); identifiers are double-quote
//! escaped exactly like upstream's `_quote_identifier`.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::core::error::{GlancesError, Result};
use crate::core::value::{self, Value};
use crate::exports::flatten::Field;

pub const NAME: &str = "duckdb";

#[derive(Debug, Clone)]
pub struct Config {
    pub database: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { database: "glances.duckdb".into() }
    }
}

/// Effective hostname id: the kernel nodename, else `localhost`.
pub fn effective_hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

/// Locate the `duckdb` CLI without a shell.
pub fn duckdb_bin() -> Option<String> {
    for dir in ["/usr/bin", "/bin", "/usr/local/bin"] {
        let full = format!("{}/duckdb", dir);
        if std::path::Path::new(&full).is_file() {
            return Some(full);
        }
    }
    None
}

/// Quote a SQL identifier (upstream `_quote_identifier` parity).
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Render a value as a SQL literal (upstream `convert_types` parity:
/// everything non-numeric/non-bool is VARCHAR, lists comma-joined).
pub fn sql_literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(true) => "TRUE".to_string(),
        Value::Bool(false) => "FALSE".to_string(),
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

/// Map a value to its DuckDB column type.
pub fn sql_type(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "BOOLEAN",
        Value::Int(_) | Value::Uint(_) => "BIGINT",
        Value::Float(_) => "DOUBLE",
        _ => "VARCHAR",
    }
}

/// One plugin table script: CREATE + INSERT statements.
#[derive(Debug, Clone, PartialEq)]
pub struct TableScript {
    pub table: String,
    pub create: String,
    pub insert: Option<String>,
}

/// Group flattened fields by plugin into per-table scripts. Column union
/// spans all rows (missing cells are NULL); element series gain a `key`
/// column. `sensors`/`fs` keep upstream's exclusion note: their varying
/// field lists still work here via the union, so they are included.
pub fn build_scripts(fields: &[Field<'_>], hostname: &str, ts: i64) -> Vec<TableScript> {
    let mut order: Vec<&str> = Vec::new();
    let mut groups: BTreeMap<&str, Vec<&Field<'_>>> = BTreeMap::new();
    for f in fields {
        if !order.contains(&f.plugin) {
            order.push(f.plugin);
        }
        groups.entry(f.plugin).or_default().push(f);
    }
    let mut out = Vec::new();
    for plugin in order {
        let group = &groups[plugin];
        let has_key = group.iter().any(|f| f.elem.is_some());
        let mut cols: Vec<(String, &'static str)> = Vec::new();
        for f in group.iter() {
            if !cols.iter().any(|(k, _)| k == f.key) {
                cols.push((f.key.to_string(), sql_type(f.value)));
            }
        }
        let mut elems: Vec<Option<String>> = Vec::new();
        for f in group.iter() {
            if !elems.contains(&f.elem) {
                elems.push(f.elem.clone());
            }
        }
        let mut header = vec![
            "\"time\" TIMESTAMPTZ".to_string(),
            "\"hostname\" VARCHAR".to_string(),
        ];
        if has_key {
            header.push("\"key\" VARCHAR".to_string());
        }
        for (name, typ) in &cols {
            header.push(format!("{} {}", quote_ident(name), typ));
        }
        let create = format!(
            "CREATE TABLE IF NOT EXISTS {} ({});",
            quote_ident(plugin),
            header.join(", ")
        );
        let mut rows = Vec::new();
        for elem in &elems {
            let mut cells = vec![
                format!("to_timestamp({})", ts),
                format!("'{}'", hostname.replace('\'', "''")),
            ];
            if has_key {
                cells.push(format!(
                    "'{}'",
                    elem.clone().unwrap_or_default().replace('\'', "''")
                ));
            }
            for (name, _) in &cols {
                let v = group
                    .iter()
                    .find(|f| &f.elem == elem && f.key == name.as_str())
                    .map(|f| (*f.value).clone())
                    .unwrap_or(Value::Null);
                cells.push(sql_literal(&v));
            }
            rows.push(format!("({})", cells.join(", ")));
        }
        let insert = if rows.is_empty() {
            None
        } else {
            let mut header_cols = vec!["\"time\"".to_string(), "\"hostname\"".to_string()];
            if has_key {
                header_cols.push("\"key\"".to_string());
            }
            for (name, _) in &cols {
                header_cols.push(quote_ident(name));
            }
            Some(format!(
                "INSERT INTO {} ({}) VALUES {};",
                quote_ident(plugin),
                header_cols.join(", "),
                rows.join(", ")
            ))
        };
        out.push(TableScript { table: plugin.to_string(), create, insert });
    }
    out
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let bin = duckdb_bin()
        .ok_or_else(|| GlancesError::Other("duckdb: binary not found".into()))?;
    let scripts = build_scripts(fields, &effective_hostname(), now_secs());
    if scripts.is_empty() {
        return Ok(());
    }
    let mut sql = String::new();
    for s in &scripts {
        sql.push_str(&s.create);
        sql.push('\n');
        if let Some(insert) = &s.insert {
            sql.push_str(insert);
            sql.push('\n');
        }
    }
    // No shell: argv-only spawn with the script on stdin.
    let mut child = Command::new(bin)
        .arg(&cfg.database)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // Always reap the child even if the script fails to send.
        let sent = stdin.write_all(sql.as_bytes());
        drop(stdin);
        let out = child.wait_with_output()?;
        sent?;
        if !out.status.success() {
            return Err(GlancesError::Other(format!(
                "duckdb: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        return Ok(());
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(GlancesError::Other(format!(
            "duckdb: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}
