//! M14 HTTP server — REST API + Vue SPA + SSE + HTTP Basic auth.
//!
//! Mirrors `glances/outputs/web/` from Python Glances: a single
//! `pub fn run` entry point that opens a `TcpListener` and serves
//! until the process is killed. The actual serving loop lives in
//! `server.rs`; this file is just the wiring + dispatch from `main.rs`.

use std::io;
use std::net::TcpListener;
use std::sync::Arc;

use crate::cli::args::Args;
use crate::core::password::PasswordFile;
use crate::core::stats::GlancesStats;

pub mod auth;
pub mod health;
pub mod meta;
pub mod mutate;
pub mod request;
pub mod response;
pub mod router;
pub mod server;
pub mod sse;
pub mod static_fs;

/// Public entry point called by `main.rs` when `args.mode == Mode::WebServer`.
///
/// `stats` is the plugin container; it must be `Arc`-wrapped because the
/// accept loop spawns one thread per connection and each thread needs a
/// `'static` reference to it.
/// `password_file` is `None` when auth is disabled.
pub fn run(stats: Arc<GlancesStats>, args: &Args, password_file: Option<PasswordFile>) -> io::Result<()> {
    let listener = TcpListener::bind((args.bind_address.as_str(), args.web_port))?;
    let pw = password_file.unwrap_or_else(PasswordFile::empty);
    let (api_key, key_src) = resolve_api_key();
    if api_key.is_some() {
        crate::core::logger::info(&format!("API key gate active (key from {})", key_src));
    }
    let state = Arc::new(server::ServerState::new(stats, args.clone(), pw, api_key));
    server::serve(listener, state)
}

/// Resolve the API key: explicit `GLANCES_API_KEY` wins, else the
/// installer's 0600 key file (first line). Returns the key plus where
/// it came from (for the startup log — never the value). Unset/blank
/// everywhere means the gate stays off.
pub fn resolve_api_key() -> (Option<String>, &'static str) {
    if let Some(k) = std::env::var(auth::API_KEY_ENV).ok().and_then(clean_key) {
        return (Some(k), "GLANCES_API_KEY");
    }
    // Installer's 0600 file, alongside glances.conf/glances.pwd
    // (same user_dir() convention as the password default path).
    let path = crate::core::config_dir::user_dir().join("api-key");
    if let Some(k) = read_key_file(&path) {
        return (Some(k), "key file");
    }
    (None, "none")
}

/// Trimmed key, or `None` when blank (empty/whitespace-only).
fn clean_key(raw: String) -> Option<String> {
    let t = raw.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

/// First line of `path`, cleaned. Missing/unreadable/blank = no key.
fn read_key_file(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    clean_key(text.lines().next().unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qa::harness::TempDir;

    #[test]
    fn clean_key_trims_and_rejects_blank() {
        assert_eq!(clean_key("  k3y  ".to_string()), Some("k3y".to_string()));
        assert_eq!(clean_key(String::new()), None);
        assert_eq!(clean_key("   ".to_string()), None);
    }

    #[test]
    fn read_key_file_takes_first_line() {
        let dir = TempDir::new("web-key-file");
        let root = dir.path();
        let p = root.join("api-key");
        std::fs::write(&p, "k3y\nsecond\n").unwrap();
        assert_eq!(read_key_file(&p), Some("k3y".to_string()));
        std::fs::write(&p, "   \n").unwrap();
        assert_eq!(read_key_file(&p), None);
        assert_eq!(read_key_file(&root.join("missing")), None);
    }
}
