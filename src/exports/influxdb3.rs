//! InfluxDB 3 exporter — `POST /api/v2/write?bucket=<bucket>` with
//! `Authorization: Token <token>` (InfluxDB 3 Core/Enterprise's v2-compat
//! write endpoint; no org parameter — v3 dropped it). When `file` is
//! set, the LP body is appended to that path instead.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::exports::flatten::Field;

pub const NAME: &str = "influxdb3";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub bucket: String,
    pub token: String,
    /// When set, append the LP body to this file instead of POSTing.
    pub file: Option<String>,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8181,
            bucket: "glances".into(),
            token: String::new(),
            file: None,
            timeout_secs: 5,
        }
    }
}

/// Build the HTTP request bytes. Exposed for unit tests.
pub fn build_request(cfg: &Config, body: &str) -> Vec<u8> {
    let mut req = Vec::with_capacity(body.len() + 256);
    let path = format!("/api/v2/write?bucket={}&precision=ns", cfg.bucket);
    let host_header = format!("{}:{}", cfg.host, cfg.port);
    write!(
        req,
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n",
        path, host_header, body.len(),
    ).unwrap();
    if !cfg.token.is_empty() {
        write!(req, "Authorization: Token {}\r\n", cfg.token).unwrap();
    }
    req.extend_from_slice(b"\r\n");
    req.extend_from_slice(body.as_bytes());
    req
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    // Same line-protocol grouping as the v2 exporter.
    let body = crate::exports::influxdb2::build_body(fields, None);
    if body.is_empty() { return Ok(()); }
    if let Some(path) = cfg.file.as_ref() {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        f.write_all(body.as_bytes())?;
        return Ok(());
    }
    if cfg.bucket.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "influxdb3 exporter requires bucket".into(),
        ));
    }
    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;
    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_write_timeout(Some(timeout))?;
    s.write_all(&build_request(cfg, &body))?;
    s.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_targets_v3_write_endpoint() {
        let cfg = Config::default();
        let req = build_request(&cfg, "x");
        let raw = String::from_utf8(req).unwrap();
        assert!(raw.starts_with("POST /api/v2/write?bucket=glances&precision=ns HTTP/1.1\r\n"));
    }

    #[test]
    fn token_emits_authorization() {
        let cfg = Config { token: "t".into(), ..Default::default() };
        let req = build_request(&cfg, "x");
        let raw = String::from_utf8(req).unwrap();
        assert!(raw.contains("Authorization: Token t\r\n"));
    }
}
