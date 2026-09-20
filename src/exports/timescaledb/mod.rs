//! TimescaleDB exporter — creates one table per plugin and inserts the
//! flattened snapshot over the PostgreSQL wire protocol (v3).
//!
//! Mirrors `glances/exports/glances_timescaledb/__init__.py` (psycopg):
//! tables carry `time TIMESTAMPTZ`, `hostname_id`, an optional `key_id`
//! for element series, and one typed column per field. Types follow the
//! upstream map (bool→BOOLEAN, int→BIGINT, float→DOUBLE PRECISION,
//! str→TEXT); nested values are stored as JSON text.
//!
//! Pure standard library, so auth is limited to what the wire needs:
//! trust/cleartext/MD5 (MD5 implemented below with RFC 1321 vectors in
//! tests). SCRAM/GSS/SSPI servers get a clear error, not a hang.
//! Hypertable clauses are skipped — plain tables work on TimescaleDB and
//! on stock PostgreSQL alike.

use crate::core::error::Result;
use crate::exports::flatten::Field;

pub const NAME: &str = "timescaledb";

/// Default PostgreSQL port.
pub const DEFAULT_PORT: u16 = 5432;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub db: String,
    pub user: String,
    pub password: String,
    pub hostname: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: DEFAULT_PORT,
            db: "glances".into(),
            user: String::new(),
            password: String::new(),
            hostname: String::new(),
            timeout_secs: 5,
        }
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Effective login user: explicit config, else $USER/$LOGNAME, else
/// `postgres` (libpq falls back to the OS user; we must send something).
pub fn effective_user(cfg: &Config) -> String {
    if !cfg.user.is_empty() {
        return cfg.user.clone();
    }
    for key in ["USER", "LOGNAME"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                return v;
            }
        }
    }
    "postgres".to_string()
}

/// Effective hostname id: explicit config, else the kernel nodename.
pub fn effective_hostname(cfg: &Config) -> String {
    if !cfg.hostname.is_empty() {
        return cfg.hostname.clone();
    }
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

/// Quote a SQL identifier with double quotes (doubles embedded quotes).

mod md5;
mod sql;
mod wire;

pub use md5::{md5_digest, md5_hex};
pub use sql::{build_tables, create_table_sql, insert_sql, quote_ident, sql_literal, sql_type, Table};
use wire::{exec_simple, pg_connect};

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let tables = build_tables(fields);
    if tables.is_empty() {
        return Ok(());
    }
    let user = effective_user(cfg);
    let hostname = effective_hostname(cfg);
    let ts = now_secs();
    let mut conn = pg_connect(cfg, &user)?;
    for t in &tables {
        exec_simple(&mut conn.stream, &create_table_sql(t))?;
        if let Some(insert) = insert_sql(t, &hostname, ts) {
            exec_simple(&mut conn.stream, &insert)?;
        }
    }
    Ok(())
}
