//! RESTful exporter — POST a JSON snapshot over HTTP/1.1 to a configured
//! endpoint. Each call opens a fresh TCP connection; failures bubble up
//! to the refresh loop, which retries naturally on the next tick (an
//! in-write backoff sleep would stall every *other* export target too).

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::{to_json, Value};

pub const NAME: &str = "restful";

#[derive(Debug, Clone)]
pub struct Config {
    /// Hostname (will be resolved with `ToSocketAddrs`).
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// HTTP request path (e.g. `/api/v1/snapshot`).
    pub path: String,
    /// Optional `Authorization: Bearer <token>` header.
    pub auth_token: Option<String>,
    /// Connection timeout (seconds). Default 5.
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
            path: "/api/v1/snapshot".into(),
            auth_token: None,
            timeout_secs: 5,
        }
    }
}

/// POST a JSON snapshot to the configured endpoint.
pub fn write(snap: &Value, cfg: &Config) -> Result<()> {
    let body = to_json(snap);
    let request = build_request(cfg, body.as_bytes());

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter
        .next()
        .ok_or_else(|| GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host)))?;

    try_post(&addr, cfg.timeout_secs, &request)
}

fn build_request(cfg: &Config, body: &[u8]) -> Vec<u8> {
    let mut req = Vec::with_capacity(body.len() + 256);
    let path = if cfg.path.is_empty() { "/" } else { cfg.path.as_str() };
    let host_header = if cfg.host.contains(':') {
        format!("[{}]:{}", cfg.host, cfg.port)
    } else {
        format!("{}:{}", cfg.host, cfg.port)
    };

    write!(
        req,
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        path, host_header, body.len()
    ).unwrap();
    if let Some(tok) = cfg.auth_token.as_ref() {
        write!(req, "Authorization: Bearer {}\r\n", tok).unwrap();
    }
    req.extend_from_slice(b"\r\n");
    req.extend_from_slice(body);
    req
}

fn try_post(addr: &std::net::SocketAddr, timeout_secs: u64, req: &[u8]) -> Result<()> {
    let timeout = Duration::from_secs(timeout_secs);
    let stream = TcpStream::connect_timeout(addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_read_timeout(Some(timeout))?;
    let mut s = stream;
    s.write_all(req)?;
    s.flush()?;
    Ok(())
}