//! HTTP Basic auth — header parsing + PasswordFile verification.
//!
//! Mirrors the Python Glances `glances/auth.py` flow: parse the
//! `Authorization: Basic base64(user:pass)` header, look up `user` in
//! the password file, and verify the SHA-256 hash (plain or salted).
//!
//! We deliberately use `PasswordFile::check` (already in core/) for the
//! crypto side. This module just bridges HTTP ↔ PasswordFile.

use super::request::Request;
use crate::core::password::PasswordFile;

const BASIC_PREFIX: &str = "Basic ";

/// Env var carrying the API key (`None`/empty = key gate off).
pub const API_KEY_ENV: &str = "GLANCES_API_KEY";
/// Request header carrying the key (header names parse lowercase).
const API_KEY_HEADER: &str = "x-api-key";

pub enum AuthOutcome { Ok, Missing, Bad }

pub fn auth_header_ok(req: &Request, pw: &PasswordFile) -> AuthOutcome {
    let header = match req.headers.get("authorization") {
        Some(h) => h,
        None => return AuthOutcome::Missing,
    };
    match parse_basic(header) {
        Some((u, p)) if verify(pw, &u, &p) => AuthOutcome::Ok,
        _ => AuthOutcome::Bad,
    }
}

/// Whether `path` needs credentials. The favicon is always open
/// (browsers fetch it alone). In key-only mode the dashboard shell
/// (`/`, `/index.html`, `/dashboard`) stays open so it can prompt
/// for the key — the shell carries no live data (skeleton + JS).
pub fn gate_applies(path: &str, basic_on: bool, key_on: bool) -> bool {
    if path == "/favicon.ico" {
        return false;
    }
    if !(basic_on || key_on) {
        return false;
    }
    if key_on && !basic_on && matches!(path, "/" | "/index.html" | "/dashboard") {
        return false;
    }
    true
}

/// True when either configured credential validates. Basic and key
/// are independent: with both on, either one passes.
pub fn credentials_ok(
    req: &Request,
    pw: &PasswordFile,
    basic_on: bool,
    api_key: Option<&str>,
) -> bool {
    let basic_ok = basic_on && matches!(auth_header_ok(req, pw), AuthOutcome::Ok);
    let key_ok = match api_key {
        Some(k) if !k.is_empty() => key_header_ok(req, k),
        _ => false,
    };
    basic_ok || key_ok
}

/// True when `X-API-Key` matches `expected` (constant-time).
/// Missing or blank header never matches.
pub fn key_header_ok(req: &Request, expected: &str) -> bool {
    match req.headers.get(API_KEY_HEADER) {
        Some(v) => {
            let v = v.trim();
            !v.is_empty() && keys_equal(v, expected)
        }
        None => false,
    }
}

/// Constant-time string equality: no early exit on first mismatch,
/// so a wrong key leaks nothing about the right one beyond length.
pub fn keys_equal(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    if x.len() != y.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..x.len() {
        diff |= x[i] ^ y[i];
    }
    diff == 0
}

/// Parse a single Authorization header value. Returns `(user, pass)` if
/// it looks like a well-formed Basic challenge, `None` otherwise.
///
/// The base64 alphabet accepted here is the standard one (`A-Z a-z 0-9 + /`)
/// with `=` padding. We decode leniently: any non-alphabet char is treated
/// as a parse failure rather than a 401 (we want clean error semantics).
pub fn parse_basic(header_value: &str) -> Option<(String, String)> {
    let raw = header_value.strip_prefix(BASIC_PREFIX)?;
    let decoded_bytes = decode_base64(raw)?;
    let s = std::str::from_utf8(&decoded_bytes).ok()?;
    let (user, pass) = s.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

/// RFC 4648 §4 base64 decoder, std-only. `=` is accepted only as 1–2
/// bytes of trailing padding — anywhere else is a parse failure, so
/// inputs like `"QQ==QQ=="` can't alias onto valid credentials.
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    if s.len() % 4 != 0 { return None; }
    let bytes = s.as_bytes();
    let pad = bytes.iter().rev().take_while(|&&b| b == b'=').count();
    if pad > 2 { return None; }
    let data = &bytes[..bytes.len() - pad];
    if data.contains(&b'=') { return None; }
    // Canonical padding: for one pad byte the last sextet's low 4 bits
    // must be zero; for two, the low 2 bits.
    if let Some(&last) = data.last() {
        let v = sextet(last)?;
        let mask = match pad { 1 => 0x0F, 2 => 0x03, _ => 0 };
        if v & mask != 0 { return None; }
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &b in data {
        let v = sextet(b)?;
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn sextet(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Verify credentials against a password file. Returns true iff the user
/// exists AND the password matches the stored hash.
pub fn verify(pw: &PasswordFile, user: &str, pass: &str) -> bool {
    pw.check(user, pass)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_basic_aladdin() {
        // RFC 7617 §2 example: Aladdin / open sesame
        let (u, p) = parse_basic("Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==").unwrap();
        assert_eq!(u, "Aladdin");
        assert_eq!(p, "open sesame");
    }
    #[test]
    fn rejects_non_basic() { assert!(parse_basic("Bearer foo").is_none()); }
    #[test]
    fn keys_equal_matches_exact_only() {
        assert!(keys_equal("abc123", "abc123"));
        assert!(!keys_equal("abc123", "abc124"));
        assert!(!keys_equal("abc123", "abc12"));
        assert!(!keys_equal("abc123", "abc1234"));
        assert!(!keys_equal("", "abc123"));
    }
    fn keyed_req(value: Option<&str>) -> Request {
        let mut headers = std::collections::HashMap::new();
        if let Some(v) = value {
            headers.insert(API_KEY_HEADER.to_string(), v.to_string());
        }
        Request { method: "GET".into(), path: "/api/4/cpu".into(), query: String::new(),
                  version: "HTTP/1.1".into(), headers, body: vec![] }
    }
    #[test]
    fn key_header_ok_accepts_match_only() {
        assert!(key_header_ok(&keyed_req(Some("s3cret")), "s3cret"));
        assert!(key_header_ok(&keyed_req(Some("  s3cret  ")), "s3cret"));
        assert!(!key_header_ok(&keyed_req(Some("wrong")), "s3cret"));
        assert!(!key_header_ok(&keyed_req(Some("")), "s3cret"));
        assert!(!key_header_ok(&keyed_req(Some("   ")), "s3cret"));
        assert!(!key_header_ok(&keyed_req(None), "s3cret"));
    }
    #[test]
    fn gate_applies_matrix() {
        // Nothing configured: everything open.
        assert!(!gate_applies("/api/4/cpu", false, false));
        // Favicon always open, even fully gated.
        assert!(!gate_applies("/favicon.ico", true, true));
        // Basic mode gates the shell too (browser prompts natively).
        assert!(gate_applies("/", true, false));
        assert!(gate_applies("/api/4/cpu", true, false));
        // Key-only mode leaves the shell open so it can ask for the key.
        assert!(!gate_applies("/", false, true));
        assert!(!gate_applies("/dashboard", false, true));
        assert!(gate_applies("/api/4/cpu", false, true));
        assert!(gate_applies("/openapi.json", false, true));
        // Both on: shell gated, Basic prompt covers browsers.
        assert!(gate_applies("/", true, true));
    }
}
