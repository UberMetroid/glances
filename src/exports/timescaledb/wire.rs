//! PostgreSQL wire protocol (v3, frontend side): connect,
//! trust/cleartext/MD5 auth, and simple-query execution.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};

use super::{md5_digest, md5_hex, Config};

// ---------------------------------------------------------------------------
// PostgreSQL wire protocol (v3, frontend side).
// ---------------------------------------------------------------------------

pub(crate) struct PgConn {
    pub(crate) stream: TcpStream,
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
pub(crate) fn exec_simple(s: &mut TcpStream, sql: &str) -> Result<()> {
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
pub(crate) fn pg_connect(cfg: &Config, user: &str) -> Result<PgConn> {
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
