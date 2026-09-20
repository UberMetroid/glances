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

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::{self, Value};
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
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|_| "localhost".to_string())
}

/// Quote a SQL identifier with double quotes (doubles embedded quotes).
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

// ---------------------------------------------------------------------------
// Minimal MD5 (RFC 1321) — only needed for PostgreSQL MD5 auth.
// ---------------------------------------------------------------------------

const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, //
    5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, //
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, //
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

fn md5_t() -> [u32; 64] {
    let mut t = [0u32; 64];
    for (i, v) in t.iter_mut().enumerate() {
        *v = ((1u64 << 32) as f64 * (i as f64 + 1.0).sin().abs()) as u64 as u32;
    }
    t
}

/// Raw MD5 digest of `msg`.
pub fn md5_digest(msg: &[u8]) -> [u8; 16] {
    let t = md5_t();
    let mut state = [0x6745_2301u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    let mut padded = msg.to_vec();
    let bit_len = (msg.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());
    for block in padded.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
        }
        let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let sum = a
                .wrapping_add(f)
                .wrapping_add(t[i])
                .wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(sum.rotate_left(MD5_S[i]));
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, s) in state.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&s.to_le_bytes());
    }
    out
}

/// Lowercase hex MD5 (RFC 1321 test vectors live in unit tests).
pub fn md5_hex(msg: &[u8]) -> String {
    md5_digest(msg).iter().map(|b| format!("{:02x}", b)).collect()
}

// ---------------------------------------------------------------------------
// PostgreSQL wire protocol (v3, frontend side).
// ---------------------------------------------------------------------------

struct PgConn {
    stream: TcpStream,
}

fn read_exact(s: &mut TcpStream, n: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf)?;
    Ok(buf)
}

/// Read one backend message: (type byte, body without the length word).
fn read_msg(s: &mut TcpStream) -> Result<(u8, Vec<u8>)> {
    let t = read_exact(s, 1)?[0];
    let len = i32::from_be_bytes(read_exact(s, 4)?[0..4].try_into().map_err(|_| {
        GlancesError::Parse("pg: short length".into())
    })?) as usize;
    if len < 4 {
        return Err(GlancesError::Parse("pg: bad length".into()));
    }
    Ok((t, read_exact(s, len - 4)?))
}

fn pg_error(body: &[u8]) -> String {
    // ErrorResponse fields: code byte + NUL-terminated string; 'M' is primary.
    let mut msg = String::new();
    let mut i = 0;
    while i < body.len() {
        let code = body[i];
        i += 1;
        let end = body[i..].iter().position(|&b| b == 0).map(|p| i + p).unwrap_or(body.len());
        let text = String::from_utf8_lossy(&body[i..end]).into_owned();
        if code == b'M' {
            msg = text;
        } else if msg.is_empty() && code != 0 {
            msg = text;
        }
        i = end + 1;
        if code == 0 {
            break;
        }
    }
    if msg.is_empty() {
        "unknown server error".to_string()
    } else {
        msg
    }
}

fn send_query(s: &mut TcpStream, sql: &str) -> Result<()> {
    let mut msg = vec![b'Q'];
    let body = [sql.as_bytes(), &[0]].concat();
    write_i32_vec(&mut msg, (body.len() + 4) as i32);
    msg.extend_from_slice(&body);
    s.write_all(&msg)?;
    s.flush()?;
    Ok(())
}

fn write_i32_vec(v: &mut Vec<u8>, n: i32) {
    v.extend_from_slice(&n.to_be_bytes());
}

/// Run one statement, draining until ReadyForQuery. Errors become Err.
fn exec_simple(s: &mut TcpStream, sql: &str) -> Result<()> {
    send_query(s, sql)?;
    loop {
        let (t, body) = read_msg(s)?;
        match t {
            b'Z' => return Ok(()),
            b'E' => return Err(GlancesError::Other(format!("pg: {}", pg_error(&body)))),
            _ => {}
        }
    }
}

/// Connect + authenticate. Supports trust (0), cleartext (3), MD5 (5).
fn pg_connect(cfg: &Config, user: &str) -> Result<PgConn> {
    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter
        .next()
        .ok_or_else(|| GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host)))?;
    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;

    // SSLRequest: decline TLS (server 'S' would require a TLS stack).
    let mut req = Vec::new();
    write_i32_vec(&mut req, 8);
    write_i32_vec(&mut req, 80877103);
    s.write_all(&req)?;
    s.flush()?;
    let resp = read_exact(&mut s, 1)?[0];
    if resp == b'S' {
        return Err(GlancesError::Other("pg: server requires TLS".into()));
    }

    // StartupMessage.
    let mut params = Vec::new();
    params.extend_from_slice(b"user\0");
    params.extend_from_slice(user.as_bytes());
    params.push(0);
    params.extend_from_slice(b"database\0");
    params.extend_from_slice(cfg.db.as_bytes());
    params.push(0);
    params.extend_from_slice(b"application_name\0glances-rs\0");
    params.push(0);
    let mut startup = Vec::new();
    write_i32_vec(&mut startup, (params.len() + 8) as i32);
    write_i32_vec(&mut startup, 196608);
    startup.extend_from_slice(&params);
    s.write_all(&startup)?;
    s.flush()?;

    // Auth loop until ReadyForQuery.
    loop {
        let (t, body) = read_msg(&mut s)?;
        match t {
            b'R' => {
                if body.len() < 4 {
                    return Err(GlancesError::Parse("pg: short auth".into()));
                }
                let code = i32::from_be_bytes(body[0..4].try_into().map_err(|_| {
                    GlancesError::Parse("pg: short auth code".into())
                })?);
                match code {
                    0 => {}
                    3 => {
                        let mut msg = vec![b'p'];
                        let pw = [cfg.password.as_bytes(), &[0]].concat();
                        write_i32_vec(&mut msg, (pw.len() + 4) as i32);
                        msg.extend_from_slice(&pw);
                        s.write_all(&msg)?;
                        s.flush()?;
                    }
                    5 => {
                        if body.len() < 8 {
                            return Err(GlancesError::Parse("pg: short md5 salt".into()));
                        }
                        let inner = md5_digest(format!("{}{}", cfg.password, user).as_bytes());
                        let mut outer_input = inner.to_vec();
                        outer_input.extend_from_slice(&body[4..8]);
                        let resp = format!("md5{}", md5_hex(&outer_input));
                        let mut msg = vec![b'p'];
                        let pw = [resp.as_bytes(), &[0]].concat();
                        write_i32_vec(&mut msg, (pw.len() + 4) as i32);
                        msg.extend_from_slice(&pw);
                        s.write_all(&msg)?;
                        s.flush()?;
                    }
                    10 => {
                        return Err(GlancesError::Other(
                            "pg: SCRAM auth unsupported (use trust/md5)".into(),
                        ))
                    }
                    _ => {
                        return Err(GlancesError::Other(format!(
                            "pg: unsupported auth method {}",
                            code
                        )))
                    }
                }
            }
            b'E' => return Err(GlancesError::Other(format!("pg: {}", pg_error(&body)))),
            b'Z' => return Ok(PgConn { stream: s }),
            _ => {}
        }
    }
}

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
