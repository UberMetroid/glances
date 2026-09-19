//! XML-RPC server + client transport (`-s` / `-c` modes).
//!
//! The server accepts one request per connection on a bounded
//! thread-per-connection model and speaks both real HTTP POST framing
//! (what Python Glances' `xmlrpc.client` sends to `/RPC2`) and raw XML
//! bodies. The client issues a single HTTP `getAll` call and prints the
//! response body.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::cli::args::Args;
use crate::core::logger;
use crate::core::stats::GlancesStats;
use crate::outputs::xmlrpc;

/// Upper bound on one XML-RPC request (headers + body) — 256 KiB is far
/// above any `getAll`/`getPlugin` call while capping memory per conn.
const MAX_REQUEST: usize = 256 * 1024;
/// Concurrent connection cap for the XML-RPC server.
const MAX_CONNS: usize = 64;

/// Run the XML-RPC server forever (one thread per accepted connection).
pub fn run_server(stats: Arc<GlancesStats>, args: &Args) {
    let bind = (args.bind_address.clone(), args.server_port);
    let listener = match TcpListener::bind(bind.clone()) {
        Ok(l) => l,
        Err(e) => {
            logger::error(&format!("xmlrpc bind {:?} failed: {}", bind, e));
            return;
        }
    };
    logger::info(&format!("xmlrpc server listening on {}:{}", bind.0, bind.1));
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let s = match stream {
            Ok(s) => s,
            Err(e) => { logger::warning(&format!("accept failed: {}", e)); continue; }
        };
        if active.load(Ordering::SeqCst) >= MAX_CONNS {
            drop(s);
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let stats = Arc::clone(&stats);
        let active = Arc::clone(&active);
        std::thread::spawn(move || {
            struct Guard(Arc<AtomicUsize>);
            impl Drop for Guard { fn drop(&mut self) { self.0.fetch_sub(1, Ordering::SeqCst); } }
            let _guard = Guard(active);
            handle_conn(s, &stats);
        });
    }
}

fn handle_conn(mut s: TcpStream, stats: &GlancesStats) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let Some(raw) = read_request(&mut s) else { return };
    let (body, http) = match raw.find("\r\n\r\n") {
        Some(i) if raw.starts_with("POST") || raw.starts_with("GET") => (&raw[i + 4..], true),
        _ => (raw.as_str(), false),
    };
    let response = xmlrpc::handle(body, stats);
    let out = if http {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        ) + std::str::from_utf8(&response).unwrap_or("")
    } else {
        String::from_utf8_lossy(&response).into_owned()
    };
    if let Err(e) = s.write_all(out.as_bytes()) {
        logger::warning(&format!("xmlrpc write failed: {}", e));
    }
}

/// Read one XML-RPC request: raw XML ends at `</methodCall>`; an HTTP
/// request ends at headers + Content-Length body. Bounded to
/// MAX_REQUEST bytes; returns `None` on timeout or oversize.
fn read_request(s: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let text = String::from_utf8_lossy(&buf).into_owned();
        let done = if text.starts_with("POST") || text.starts_with("GET") {
            match text.find("\r\n\r\n") {
                None => false,
                Some(i) => buf.len() >= i + 4 + content_length(&text[..i]),
            }
        } else {
            text.contains("</methodCall>")
        };
        if done { return Some(text); }
        if buf.len() >= MAX_REQUEST { return None; }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return if buf.is_empty() { None } else { Some(text) },
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

fn content_length(headers: &str) -> usize {
    headers.lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0)
}

/// Run the XML-RPC client: HTTP POST a single `getAll` call to `/RPC2`
/// and print the response body.
pub fn run_client(host: &str, port: u16) {
    let payload = b"<?xml version=\"1.0\"?><methodCall><methodName>getAll</methodName><params/></methodCall>";
    let req = format!(
        "POST /RPC2 HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        host, port, payload.len()
    );
    let mut s = match TcpStream::connect((host, port)) {
        Ok(s) => s,
        Err(e) => { println!("connect failed: {}", e); return; }
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    if let Err(e) = s.write_all(req.as_bytes()) { println!("write failed: {}", e); return; }
    if let Err(e) = s.write_all(payload) { println!("write failed: {}", e); return; }
    let mut buf = Vec::new();
    if let Err(e) = s.read_to_end(&mut buf) { println!("read failed: {}", e); return; }
    let text = String::from_utf8_lossy(&buf);
    // Strip HTTP headers if the server framed the response.
    let body = match text.find("\r\n\r\n") {
        Some(i) if text.starts_with("HTTP/") => &text[i + 4..],
        _ => &text[..],
    };
    println!("{}", body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_length_parsed_case_insensitively() {
        assert_eq!(content_length("POST / RPC2\r\nContent-Length: 42"), 42);
        assert_eq!(content_length("content-length:7"), 7);
        assert_eq!(content_length("no header"), 0);
    }

    #[test]
    fn server_accepts_http_post_and_replies_http() {
        use std::io::Read;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stats = Arc::new(GlancesStats::new(2.0));
        crate::plugins::register_all(&stats);
        let s2 = Arc::clone(&stats);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if let Ok(s) = stream { handle_conn(s, &s2); }
            }
        });
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = b"<?xml version=\"1.0\"?><methodCall><methodName>getAllPlugins</methodName><params/></methodCall>";
        let req = format!(
            "POST /RPC2 HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        c.write_all(req.as_bytes()).unwrap();
        c.write_all(body).unwrap();
        let mut resp = Vec::new();
        c.read_to_end(&mut resp).unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200"), "got: {}", text);
        assert!(text.contains("<methodResponse>"));
        assert!(!text.contains("<value><value>"), "double wrap: {}", text);
    }
}
