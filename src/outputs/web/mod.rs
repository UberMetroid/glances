//! Web server: REST API, dashboard shell, event stream, auth gates.
//!
//! One `run` entry binds the listener and serves until the process
//! exits. The accept loop lives in `server`; routing in `router`.

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

/// Serve forever on the configured address and port. `stats` is shared
/// across connection threads; auth is off without a credential file.
pub fn run(stats: Arc<GlancesStats>, args: &Args, password_file: Option<PasswordFile>) -> io::Result<()> {
    let listener = TcpListener::bind((args.bind_address.as_str(), args.web_port))?;
    let pw = password_file.unwrap_or_else(PasswordFile::empty);
    let (api_key, key_src) = resolve_api_key();
    if api_key.is_some() {
        crate::core::logger::info(&format!("API key gate active (key from {key_src})"));
    }
    server::serve(listener, Arc::new(server::ServerState::new(stats, args.clone(), pw, api_key)))
}

/// The API key plus where it came from (for the startup log — never
/// the value). Explicit env wins, then the installer's 0600 key file
/// beside the user config; blank everywhere means the gate stays off.
pub fn resolve_api_key() -> (Option<String>, &'static str) {
    if let Some(k) = std::env::var(auth::API_KEY_ENV).ok().and_then(clean_key) {
        return (Some(k), "GLANCES_API_KEY");
    }
    let path = crate::core::config_dir::user_dir().join("api-key");
    if let Some(k) = read_key_file(&path) {
        return (Some(k), "key file");
    }
    (None, "none")
}

/// Trimmed key, or None when blank.
fn clean_key(raw: String) -> Option<String> {
    let t = raw.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// First line of the key file, cleaned. Missing, unreadable, or blank
/// files all mean no key.
fn read_key_file(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    clean_key(text.lines().next().unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qa::harness::TempDir;

    #[test]
    fn blank_keys_are_no_keys() {
        assert_eq!(clean_key("  k3y  ".to_string()), Some("k3y".to_string()));
        assert_eq!(clean_key(String::new()), None);
        assert_eq!(clean_key("   ".to_string()), None);
    }

    #[test]
    fn key_file_reads_first_line_only() {
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
