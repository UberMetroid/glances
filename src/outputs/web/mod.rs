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
pub mod meta;
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
    let state = Arc::new(server::ServerState::new(stats, args.clone(), pw));
    server::serve(listener, state)
}
