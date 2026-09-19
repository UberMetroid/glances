//! HTTP/1.1 request parser — minimal, std-only, no `httparse`.
//!
//! Reads a single request from a byte slice that has already been pulled
//! off the socket (header + body combined). For SSE / long-poll we still
//! want a parseable first frame, so we accept requests with or without
//! a body — `Content-Length` is honored up to `MAX_BODY`.
//!
//! Not aiming for full RFC 7230 compliance; we cover what Glances' own
//! web UI sends: GET / POST, absolute or relative URIs, plain headers,
//! optional body.

use std::collections::HashMap;

/// One parsed HTTP/1.1 request.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Maximum body bytes we'll buffer for a single request. SSE POST
/// notifications are tiny; anything larger is suspicious.
pub const MAX_BODY: usize = 64 * 1024;

/// Maximum header bytes we'll accept before dropping the connection.
/// 16 KiB is far above any sane HTTP/1.1 header block.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;

/// Parse a request. Returns `None` on malformed input.
pub fn parse(buf: &[u8]) -> Option<Request> {
    // Header/body split on the first CRLFCRLF (RFC 7230 §3.5 style).
    let split_at = find_double_crlf(buf)?;
    let (head, body) = buf.split_at(split_at);
    let head = std::str::from_utf8(head).ok()?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?;
    let (method, path_with_query, version) = parse_request_line(request_line)?;
    let (path, query) = split_path_query(&path_with_query);
    let mut headers: HashMap<String, String> = HashMap::new();
    for line in lines {
        if line.is_empty() { break; }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let body = body[4..].to_vec();
    if body.len() > MAX_BODY { return None; }
    let _ = version; // version is always HTTP/1.1 in our world; recorded but unused.
    Some(Request { method, path, query, version: "HTTP/1.1".into(), headers, body })
}

fn parse_request_line(line: &str) -> Option<(String, String, String)> {
    let mut it = line.split_whitespace();
    let m = it.next()?.to_string();
    let t = it.next()?.to_string();
    let v = it.next()?.to_string();
    if it.next().is_some() { return None; } // extra tokens → malformed
    Some((m, t, v))
}

fn split_path_query(p: &str) -> (String, String) {
    match p.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (p.to_string(), String::new()),
    }
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    // Naive linear scan. Cheap for 16 KiB; correctness over cleverness.
    if buf.len() < 4 { return None; }
    for i in 0..=buf.len() - 4 {
        if &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

/// Read a full HTTP request from a reader, returning the parsed request
/// when both the header block and (optional) body are complete.
/// Times out gracefully: `None` means "incomplete, call again with more bytes".
pub struct Reader {
    buf: Vec<u8>,
    done: bool,
}

impl Reader {
    pub fn new() -> Self { Self { buf: Vec::with_capacity(512), done: false } }
    pub fn feed(&mut self, more: &[u8]) { self.buf.extend_from_slice(more); }
    /// Expose buffer length so the accept loop can enforce the cap.
    pub fn buf_len(&self) -> usize { self.buf.len() }
    pub fn try_parse(&mut self) -> Option<Request> {
        if self.done { return None; }
        let split = match find_double_crlf(&self.buf) {
            Some(s) => s,
            None => return None,
        };
        let head = std::str::from_utf8(&self.buf[..split]).ok()?;
        let content_length: usize = head.lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
            .and_then(|(_, v)| v.trim().parse().ok())
            .unwrap_or(0);
        let total = split + 4 + content_length;
        if self.buf.len() < total { return None; }
        let req = parse(&self.buf[..total])?;
        self.buf.drain(..total);
        self.done = self.buf.is_empty();
        Some(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_simple_get() {
        let raw = b"GET /foo?x=1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let r = parse(raw).unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/foo");
        assert_eq!(r.query, "x=1");
        assert_eq!(r.headers.get("host").unwrap(), "localhost");
    }
    #[test]
    fn parses_post_with_body() {
        let raw = b"POST /api HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let r = parse(raw).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.body, b"hello");
    }
    #[test]
    fn rejects_garbage() { assert!(parse(b"not http\r\n\r\n").is_none()); }
}
