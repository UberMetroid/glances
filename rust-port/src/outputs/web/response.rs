//! HTTP/1.1 response builder.
//!
//! We hand-roll status lines + headers to keep with the AC-1 "no crates"
//! rule. Each helper (`ok_json`, `not_found`, …) returns a `Response`
//! that's later serialized to bytes by `Response::into_bytes`.

use std::collections::HashMap;

/// Common status codes we emit. Matches RFC 7231 §6 numeric codes.
pub mod status {
    pub const OK: u16 = 200;
    pub const NO_CONTENT: u16 = 204;
    pub const BAD_REQUEST: u16 = 400;
    pub const UNAUTHORIZED: u16 = 401;
    pub const NOT_FOUND: u16 = 404;
    pub const METHOD_NOT_ALLOWED: u16 = 405;
    pub const INTERNAL: u16 = 500;
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub reason: &'static str,
    pub headers: HashMap<&'static str, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, reason: &'static str) -> Self {
        Self { status, reason, headers: HashMap::new(), body: Vec::new() }
    }
    pub fn header(mut self, k: &'static str, v: impl Into<String>) -> Self {
        self.headers.insert(k, v.into());
        self
    }
    pub fn body(mut self, b: impl Into<Vec<u8>>) -> Self {
        self.body = b.into();
        self
    }
    pub fn body_str(mut self, s: &str) -> Self {
        self.body = s.as_bytes().to_vec();
        self
    }

    pub fn ok_json(s: String) -> Self {
        Response::new(status::OK, "OK")
            .header("Content-Type", "application/json; charset=utf-8")
            .header("Cache-Control", "no-store")
            .body(s.into_bytes())
    }
    pub fn ok_text(s: String) -> Self {
        Response::new(status::OK, "OK")
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(s.into_bytes())
    }
    pub fn ok_html(s: String) -> Self {
        Response::new(status::OK, "OK")
            .header("Content-Type", "text/html; charset=utf-8")
            .body(s.into_bytes())
    }
    pub fn ok_bytes(b: Vec<u8>, ct: &'static str) -> Self {
        Response::new(status::OK, "OK")
            .header("Content-Type", ct)
            .body(b)
    }
    pub fn not_found() -> Self {
        Response::new(status::NOT_FOUND, "Not Found")
            .header("Content-Type", "text/plain; charset=utf-8")
            .body_str("404 Not Found\n")
    }
    pub fn bad_request(msg: &str) -> Self {
        Response::new(status::BAD_REQUEST, "Bad Request")
            .header("Content-Type", "text/plain; charset=utf-8")
            .body_str(&format!("400 {}\n", msg))
    }
    pub fn unauthorized() -> Self {
        Response::new(status::UNAUTHORIZED, "Unauthorized")
            .header("Content-Type", "text/plain; charset=utf-8")
            .header("WWW-Authenticate", "Basic realm=\"glances\"")
            .body_str("401 Unauthorized\n")
    }
    pub fn internal_error(msg: &str) -> Self {
        Response::new(status::INTERNAL, "Internal Server Error")
            .header("Content-Type", "text/plain; charset=utf-8")
            .body_str(&format!("500 {}\n", msg))
    }
    pub fn method_not_allowed() -> Self {
        Response::new(status::METHOD_NOT_ALLOWED, "Method Not Allowed")
            .header("Allow", "GET, POST")
            .header("Content-Type", "text/plain; charset=utf-8")
            .body_str("405 Method Not Allowed\n")
    }
    pub fn no_content() -> Self { Response::new(status::NO_CONTENT, "No Content") }

    /// Serialize to wire bytes. Adds Content-Length + Date + Server
    /// automatically when missing.
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.headers.entry("Content-Length").or_insert_with(|| self.body.len().to_string());
        self.headers.entry("Server").or_insert_with(|| "glances-rs".to_string());
        self.headers.entry("Connection").or_insert_with(|| "close".to_string());
        let mut out = Vec::with_capacity(self.body.len() + 256);
        let head = format!("HTTP/1.1 {} {}\r\n", self.status, self.reason);
        out.extend_from_slice(head.as_bytes());
        for (k, v) in &self.headers {
            out.extend_from_slice(k.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(v.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_lines_match() {
        assert_eq!(Response::not_found().status, 404);
        assert_eq!(Response::unauthorized().status, 401);
    }
    #[test]
    fn into_bytes_has_content_length() {
        let r = Response::ok_text("hi".into()).into_bytes();
        let s = String::from_utf8_lossy(&r);
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("Content-Length: 2"));
        assert!(s.ends_with("hi"));
    }
}
