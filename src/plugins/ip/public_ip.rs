//! Public-IP fetch + the process-wide refresh daemon (split out for the
//! file-size lint). The daemon is bounded: at most one thread per
//! process, started only via `spawn_daemon()` from `register()`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::thread;
use std::time::Duration;

use crate::core::error::{GlancesError, Result};

/// Default endpoint to query for the public IP. We pick ipify because it
/// returns plain text, no auth, no TLS to wrestle with. Plaintext HTTP
/// is spoofable — treat the value as best-effort, not authoritative.
const PUBLIC_IP_HOST: &str = "api.ipify.org";
const PUBLIC_IP_PATH: &str = "/";

/// How long the daemon sleeps between refresh attempts.
const REFRESH_SECS: u64 = 60;

/// Process-wide shared cell every plugin instance reads.
static PUBLIC_IP: OnceLock<Arc<Mutex<String>>> = OnceLock::new();
static DAEMON: Once = Once::new();

/// The shared `Mutex<String>` holding the most recent public IP.
pub fn cell() -> Arc<Mutex<String>> {
    PUBLIC_IP.get_or_init(|| Arc::new(Mutex::new(String::new()))).clone()
}

/// Spawn the background refresh thread once per process. Idempotent —
/// subsequent calls are no-ops.
pub fn spawn_daemon() {
    DAEMON.call_once(|| {
        let arc = cell();
        thread::spawn(move || loop {
            if let Ok(ip) = fetch_public_ip() {
                if let Ok(mut guard) = arc.lock() {
                    *guard = ip;
                }
            }
            thread::sleep(Duration::from_secs(REFRESH_SECS));
        });
    });
}

/// Fetch the public IP via a plain-text HTTP GET to api.ipify.org.
/// Returns Err on any network/parse failure. Uses std::net only.
pub fn fetch_public_ip() -> Result<String> {
    let mut stream = TcpStream::connect((PUBLIC_IP_HOST, 80))
        .map_err(GlancesError::Io)?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: glances-rs/0.9\r\nConnection: close\r\n\r\n",
        PUBLIC_IP_PATH, PUBLIC_IP_HOST
    );
    stream.write_all(req.as_bytes()).map_err(GlancesError::Io)?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).map_err(GlancesError::Io)?;
    // Split headers from body at "\r\n\r\n".
    let body = match buf.find("\r\n\r\n") {
        Some(i) => &buf[i + 4..],
        None => return Err(GlancesError::Parse("no http body delimiter".into())),
    };
    let ip = body.trim().to_string();
    if ip.is_empty() {
        return Err(GlancesError::Parse("empty public ip response".into()));
    }
    Ok(ip)
}
