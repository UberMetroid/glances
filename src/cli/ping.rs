//! `--ping ADDR`: probe a running server's health endpoint and exit.
//!
//! Container `HEALTHCHECK` without shipping curl: TCP-connect to
//! `host:port` (default port 61208), `GET /api/4/health`, exit 0 on
//! a 200. Any failure (refused, timeout, non-200) exits nonzero.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Default web port when the target has no `:port` suffix.
pub const DEFAULT_PORT: u16 = 61208;

/// Split `host:port` (last colon wins, so `[::1]:61208` works). Bare
/// hosts get [`DEFAULT_PORT`]; garbage ports fall back to it too.
pub fn split_target(target: &str) -> (String, u16) {
    match target.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && !h.ends_with(':') => {
            (h.trim_matches(&['[', ']'][..]).to_string(), p.parse().unwrap_or(DEFAULT_PORT))
        }
        _ => (target.trim_matches(&['[', ']'][..]).to_string(), DEFAULT_PORT),
    }
}

/// Probe once. True only on HTTP 200 from `/api/4/health`.
pub fn ping_once(target: &str) -> bool {
    let (host, port) = split_target(target);
    let addr = match (host.as_str(), port).to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => return false,
        },
        Err(_) => return false,
    };
    let mut sock = match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = sock.set_read_timeout(Some(Duration::from_secs(3)));
    if sock.write_all(b"GET /api/4/health HTTP/1.0\r\nConnection: close\r\n\r\n").is_err() {
        return false;
    }
    let mut buf = [0u8; 1024];
    let mut head = Vec::new();
    loop {
        match sock.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                head.extend_from_slice(&buf[..n]);
                if head.len() >= 1024 || head.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return false,
        }
    }
    let text = String::from_utf8_lossy(&head);
    text.starts_with("HTTP/") && text.split_whitespace().nth(1) == Some("200")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_split_shapes() {
        assert_eq!(split_target("127.0.0.1:61208"), ("127.0.0.1".into(), 61208));
        assert_eq!(split_target("127.0.0.1"), ("127.0.0.1".into(), DEFAULT_PORT));
        assert_eq!(split_target("[::1]:1"), ("::1".into(), 1));
        assert_eq!(split_target("host:junk"), ("host".into(), DEFAULT_PORT));
    }

    #[test]
    fn live_server_pings_and_closed_port_fails() {
        use crate::cli::args::Args;
        use crate::core::password::PasswordFile;
        use crate::core::stats::GlancesStats;
        use crate::outputs::web::server::spawn_test_server;
        use std::sync::Arc;
        let (addr, _h) = spawn_test_server(
            Arc::new(GlancesStats::new(2.0)), Args::default(), PasswordFile::empty(), None,
        );
        assert!(ping_once(&addr.to_string()));
        assert!(!ping_once("127.0.0.1:1"));
    }
}
