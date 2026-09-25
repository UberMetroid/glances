//! Public-IP fetch + the process-wide refresh daemon.
//!
//! Opt-in like upstream Glances: nothing is fetched unless `[ip]`
//! `public_api` is configured (and `public_disabled` isn't true). The
//! daemon is bounded — at most one thread per process, started only
//! via `spawn_daemon()` from `register()` after `configure()` ran.
//!
//! Supported config keys (`[ip]` section):
//!   - `public_api`              — required; `http://host[:port]/path`
//!   - `public_field`            — optional comma list of JSON keys to
//!     extract (e.g. `ip,query`); when set
//!     the body must be a JSON object
//!   - `public_username`/`public_password` — HTTP Basic auth
//!   - `public_refresh_interval` — seconds between fetches (default 300)
//!   - `public_disabled`         — `true` forces the feature off
//!
//! `https://` is rejected with a warning — this crate has no TLS stack
//! and silently downgrading to plaintext is not an option.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::thread;
use std::time::Duration;

use crate::core::config::Config;
use crate::core::error::{GlancesError, Result};

/// Resolved fetch endpoint + auth + refresh interval.
#[derive(Clone)]
pub struct PublicCfg {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub fields: Vec<String>,
    pub basic_auth: Option<(String, String)>,
    pub refresh_secs: u64,
}

static PUBLIC_IP: OnceLock<Arc<Mutex<String>>> = OnceLock::new();
static CFG: OnceLock<Option<PublicCfg>> = OnceLock::new();
static DAEMON: Once = Once::new();

/// Env override for the public-IP endpoint (container-friendly).
pub const PUBLIC_API_ENV: &str = "GLANCES_PUBLIC_API";

/// Endpoint order: env wins, config falls back; blanks mean off.
pub fn resolve_api_url(cfg: Option<&str>, env: Option<&str>) -> Option<String> {
    for raw in [env, cfg].into_iter().flatten() {
        if !raw.trim().is_empty() { return Some(raw.trim().to_string()); }
    }
    None
}

/// The shared `Mutex<String>` holding the most recent public IP.
pub fn cell() -> Arc<Mutex<String>> {
    PUBLIC_IP.get_or_init(|| Arc::new(Mutex::new(String::new()))).clone()
}

/// Read `[ip]` keys once; must run before `register()`/daemon spawn.
/// Missing `public_api` (or `public_disabled=true`) disables the fetch
/// entirely — no outbound HTTP ever happens in that case.
pub fn configure(cfg: &Config) {
    CFG.get_or_init(|| {
        if cfg.get("ip", "public_disabled")
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            return None;
        }
        let env = std::env::var(PUBLIC_API_ENV).ok();
        let api = match resolve_api_url(cfg.get("ip", "public_api"), env.as_deref()) {
            Some(a) => a,
            None => return None,
        };
        let api = api.as_str();
        let parsed = parse_api_url(api);
        let (host, port, path) = match parsed {
            Some(t) => t,
            None => {
                crate::core::logger::warning(&format!(
                    "ip: public_api '{}' is not a supported http:// URL \
                     (https needs a TLS stack we don't ship) — public IP disabled",
                    api
                ));
                return None;
            }
        };
        let fields = cfg.get("ip", "public_field")
            .map(|s| s.split(',').map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty()).collect())
            .unwrap_or_default();
        let basic_auth = match (
            cfg.get("ip", "public_username"),
            cfg.get("ip", "public_password"),
        ) {
            (Some(u), Some(p)) => Some((u.to_string(), p.to_string())),
            _ => None,
        };
        let refresh_secs = cfg.get_float("ip", "public_refresh_interval")
            .map(|v| v.max(1.0) as u64)
            .unwrap_or(300);
        Some(PublicCfg { host, port, path, fields, basic_auth, refresh_secs })
    });
}

/// `http://host[:port]/path` → (host, port, path). Rejects every other
/// scheme and non-http ports-less forms.
pub fn parse_api_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()?),
        None => (authority, 80),
    };
    if host.is_empty() { return None; }
    Some((host.to_string(), port, path.to_string()))
}

/// Spawn the background refresh thread once per process — only when
/// `configure()` produced an endpoint. Idempotent.
pub fn spawn_daemon() {
    DAEMON.call_once(|| {
        let cfg = match CFG.get().and_then(|o| o.as_ref()) {
            Some(c) => c.clone(),
            None => return, // not configured → never fetch
        };
        let arc = cell();
        thread::spawn(move || loop {
            if let Ok(ip) = fetch_public_ip(&cfg)
                && let Ok(mut guard) = arc.lock() {
                    *guard = ip;
                }
            thread::sleep(Duration::from_secs(cfg.refresh_secs));
        });
    });
}

/// Fetch the public IP via HTTP GET per `cfg`. Returns Err on any
/// network/parse failure, and validates the body: either the JSON
/// `public_field` keys or a bare IP literal — never arbitrary text.
pub fn fetch_public_ip(cfg: &PublicCfg) -> Result<String> {
    let mut stream = TcpStream::connect((cfg.host.as_str(), cfg.port))
        .map_err(GlancesError::Io)?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let auth = match &cfg.basic_auth {
        Some((u, p)) => format!(
            "Authorization: Basic {}\r\n", base64_encode(format!("{}:{}", u, p).as_bytes())),
        None => String::new(),
    };
    let req = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\n{}{}Connection: close\r\n\r\n",
        cfg.path, cfg.host, auth,
        "User-Agent: glances-rs/0.10\r\n",
    );
    stream.write_all(req.as_bytes()).map_err(GlancesError::Io)?;
    // Bounded read — a hostile endpoint can't stall or OOM the thread.
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > 256 * 1024 {
                    return Err(GlancesError::Parse("public ip response too large".into()));
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) => return Err(GlancesError::Io(e)),
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let body = match text.find("\r\n\r\n") {
        Some(i) => text[i + 4..].trim().to_string(),
        None => return Err(GlancesError::Parse("no http body delimiter".into())),
    };
    if body.is_empty() {
        return Err(GlancesError::Parse("empty public ip response".into()));
    }
    extract_ip(&body, &cfg.fields)
}

/// Pick the IP out of a response body. With `fields`, try each JSON key
/// (`{"ip": "1.2.3.4"}` → key `ip`). Without, the whole trimmed body
/// must itself be a valid IPv4/IPv6 literal — an HTML error page or
/// garbage must not be stored as the public address.
pub fn extract_ip(body: &str, fields: &[String]) -> Result<String> {
    if !fields.is_empty() {
        if let Some(crate::core::value::Value::Object(obj)) =
            crate::plugins::json::parse_object(body)
        {
            for f in fields {
                if let Some(v) = obj.get(f).and_then(|v| v.as_str())
                    && is_ip_literal(v) {
                        return Ok(v.to_string());
                    }
            }
        }
        return Err(GlancesError::Parse(
            "public ip: no configured field held an IP literal".into()));
    }
    if is_ip_literal(body) {
        Ok(body.to_string())
    } else {
        Err(GlancesError::Parse("public ip body is not an IP literal".into()))
    }
}

fn is_ip_literal(s: &str) -> bool {
    let t = s.trim();
    t.parse::<std::net::Ipv4Addr>().is_ok() || t.parse::<std::net::Ipv6Addr>().is_ok()
}

/// Minimal base64 for the Basic-auth header.
fn base64_encode(input: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for c in input.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | (*c.get(2).unwrap_or(&0) as u32);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}
