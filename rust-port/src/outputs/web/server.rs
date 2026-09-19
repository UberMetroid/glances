//! TCP accept loop — one thread per connection.
//!
//! This is a deliberately simple server: no thread pool, no async, no
//! keep-alive. Python Glances' `outputs/web` uses a threaded WSGI server
//! (werkzeug's `ThreadedWSGIServer`) so we mirror that with `std::thread::spawn`.
//!
//! Each connection:
//!   1. Read up to MAX_HEADER_BYTES.
//!   2. Parse the request.
//!   3. Route → Response.
//!   4. Write response, drop the socket.
//!
//! If anything goes wrong we just close the socket — the next client
//! gets a fresh connection. This matches the "fail open, keep serving"
//! posture the rest of the codebase uses.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use super::request::{Reader, MAX_HEADER_BYTES};
use super::router::{route, Ctx};
use crate::cli::args::Args;
use crate::core::password::PasswordFile;
use crate::core::stats::GlancesStats;

/// Shared per-server state. We don't `derive(Clone)` because `PasswordFile`
/// is not `Clone`; instead we wrap it in an `Arc` for cheap fan-out.
pub struct ServerState {
    pub stats: Arc<GlancesStats>,
    pub args: Args,
    pub password: Arc<PasswordFile>,
}

impl ServerState {
    pub fn new(stats: Arc<GlancesStats>, args: Args, password: PasswordFile) -> Self {
        Self { stats, args, password: Arc::new(password) }
    }
    /// Cheap clone for handing to a worker thread.
    pub fn shared(&self) -> SharedServerState {
        SharedServerState { stats: Arc::clone(&self.stats),
                            args: self.args.clone(),
                            password: Arc::clone(&self.password) }
    }
}

/// A `Clone`-able view of `ServerState` suitable for sending to threads.
#[derive(Clone)]
pub struct SharedServerState {
    pub stats: Arc<GlancesStats>,
    pub args: Args,
    pub password: Arc<PasswordFile>,
}

/// Block on `listener` forever. Returns when the listener errors out
/// (e.g. the process is killed).
pub fn serve(listener: TcpListener, state: Arc<ServerState>) -> io::Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let shared = state.shared();
                std::thread::spawn(move || handle_conn(s, shared));
            }
            Err(e) => {
                crate::core::logger::warning(&format!("accept failed: {}", e));
                continue;
            }
        }
    }
    Ok(())
}

fn handle_conn(mut sock: TcpStream, state: SharedServerState) {
    let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let _ = sock.set_write_timeout(Some(std::time::Duration::from_secs(5)));
    let mut reader = Reader::new();
    let mut buf = [0u8; 4096];
    let req = match read_request(&mut sock, &mut reader, &mut buf) {
        Some(r) => r,
        None => return,
    };
    let refresh_seq = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let ctx = Ctx {
        stats: &state.stats,
        args: &state.args,
        password: &state.password,
        auth_enabled: state.args.auth_enabled,
        refresh_seq,
    };
    let resp = route(&req, &ctx);
    let bytes = resp.into_bytes();
    if let Err(e) = sock.write_all(&bytes) {
        crate::core::logger::warning(&format!("write failed: {}", e));
    }
    let _ = sock.flush();
}

fn read_request(sock: &mut TcpStream, reader: &mut Reader, buf: &mut [u8]) -> Option<super::request::Request> {
    loop {
        if let Some(r) = reader.try_parse() { return Some(r); }
        let n = match sock.read(buf) {
            Ok(0) => return None,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => return None,
            Err(_) => return None,
        };
        if reader.buf_len() + n > MAX_HEADER_BYTES { return None; }
        reader.feed(&buf[..n]);
    }
}

/// Test helper: run `serve` on a background thread using a free port.
/// Returns the listener address and a join handle. `stats` is moved into
/// the server (wrapped in `Arc`); the caller can register plugins on it
/// before calling this and they'll be visible to the server threads.
#[cfg(test)]
pub fn spawn_test_server(stats: Arc<GlancesStats>, args: Args) -> (std::net::SocketAddr, std::thread::JoinHandle<io::Result<()>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let addr = listener.local_addr().unwrap();
    let state = Arc::new(ServerState::new(stats, args, PasswordFile::empty()));
    let handle = std::thread::spawn(move || serve(listener, state));
    (addr, handle)
}
