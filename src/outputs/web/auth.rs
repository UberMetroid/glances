//! Request authentication: HTTP Basic plus the API-key gate.
//!
//! Basic credentials verify against the credential file; the API key
//! compares constant-time against the configured key. Either one
//! passes when both gates are on.

use super::request::Request;
use crate::core::password::PasswordFile;

const BASIC_PREFIX: &str = "Basic ";

/// Env var carrying the API key (unset/blank = gate off).
pub const API_KEY_ENV: &str = "GLANCES_API_KEY";
/// Key header (request headers parse lowercase).
const API_KEY_HEADER: &str = "x-api-key";

pub enum AuthOutcome { Ok, Missing, Bad }

pub fn auth_header_ok(req: &Request, pw: &PasswordFile) -> AuthOutcome {
    let Some(header) = req.headers.get("authorization") else {
        return AuthOutcome::Missing;
    };
    match parse_basic(header) {
        Some((u, p)) if verify(pw, &u, &p) => AuthOutcome::Ok,
        _ => AuthOutcome::Bad,
    }
}

/// Whether a path needs credentials. The favicon never does. In
/// key-only mode the dashboard shell stays open so it can prompt for
/// the key (the shell is a skeleton carrying no live data).
pub fn gate_applies(path: &str, basic_on: bool, key_on: bool) -> bool {
    if path == "/favicon.ico" || !(basic_on || key_on) {
        return false;
    }
    if key_on && !basic_on && matches!(path, "/" | "/index.html" | "/dashboard") {
        return false;
    }
    true
}

/// True when either configured credential validates.
pub fn credentials_ok(
    req: &Request,
    pw: &PasswordFile,
    basic_on: bool,
    api_key: Option<&str>,
) -> bool {
    let basic_ok = basic_on && matches!(auth_header_ok(req, pw), AuthOutcome::Ok);
    let key_ok = api_key.is_some_and(|k| !k.is_empty() && key_header_ok(req, k));
    basic_ok || key_ok
}

/// True on an exact key match. Missing and blank headers never match.
pub fn key_header_ok(req: &Request, expected: &str) -> bool {
    match req.headers.get(API_KEY_HEADER) {
        Some(v) => {
            let v = v.trim();
            !v.is_empty() && keys_equal(v, expected)
        }
        None => false,
    }
}

/// Constant-time equality: every byte always compares, so a wrong key
/// leaks nothing beyond its length.
pub fn keys_equal(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    if x.len() != y.len() {
        return false;
    }
    x.iter().zip(y.iter()).fold(0u8, |acc, (p, q)| acc | (p ^ q)) == 0
}

/// Split a Basic header into (user, password): exact `Basic ` prefix,
/// strict base64, valid UTF-8, first colon separates. Anything else is
/// not a credential (callers 401, never 500).
pub fn parse_basic(header_value: &str) -> Option<(String, String)> {
    let raw = header_value.strip_prefix(BASIC_PREFIX)?;
    let bytes = decode_base64(raw)?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

/// Strict base64: length in whole quanta, at most 2 pad chars and only
/// trailing, canonical zero tail bits (one pad byte leaves 2 spare
/// bits, two leave 4 — any set bit rejects, so non-canonical inputs
/// can't alias onto valid credentials).
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(4) {
        return None;
    }
    let bytes = s.as_bytes();
    let pad = bytes.iter().rev().take_while(|&&b| b == b'=').count();
    if pad > 2 {
        return None;
    }
    let data = &bytes[..bytes.len() - pad];
    if data.contains(&b'=') {
        return None;
    }
    if let Some(&last) = data.last() {
        let mask = match pad {
            1 => 0x03,
            2 => 0x0F,
            _ => 0,
        };
        if sextet(last)? & mask != 0 {
            return None;
        }
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (full, rest) = data.split_at(data.len() / 4 * 4);
    for q in full.chunks(4) {
        let n = (sextet(q[0])? as u32) << 18
            | (sextet(q[1])? as u32) << 12
            | (sextet(q[2])? as u32) << 6
            | sextet(q[3])? as u32;
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
        out.push(n as u8);
    }
    match rest {
        [] => {}
        [a, b] => {
            let n = (sextet(*a)? as u32) << 6 | sextet(*b)? as u32;
            out.push((n >> 4) as u8);
        }
        [a, b, c] => {
            let n = (sextet(*a)? as u32) << 12 | (sextet(*b)? as u32) << 6 | sextet(*c)? as u32;
            out.push((n >> 10) as u8);
            out.push((n >> 2) as u8);
        }
        _ => return None,
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

pub fn verify(pw: &PasswordFile, user: &str, pass: &str) -> bool {
    pw.check(user, pass)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc7617_aladdin_vector() {
        let (u, p) = parse_basic("Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==").unwrap();
        assert_eq!((u.as_str(), p.as_str()), ("Aladdin", "open sesame"));
    }
    #[test]
    fn malformed_headers_are_not_credentials() {
        assert!(parse_basic("Bearer [REDACTED]").is_none());
        assert!(parse_basic("Basic !!!").is_none());
        assert!(parse_basic("Basic TWFu").is_none());
        assert!(parse_basic("basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==").is_none());
    }
    #[test]
    fn canonical_padding_only() {
        // "TWE=" is canonical "Ma" and must decode.
        assert_eq!(decode_base64("TWE=").unwrap(), b"Ma");
        // "AE==" carries set pad bits and must not.
        assert!(decode_base64("AE==").is_none());
        assert!(decode_base64("QQ==QQ==").is_none());
        assert!(decode_base64("QQQ=").is_some());
    }
    #[test]
    fn key_equality_is_exact() {
        assert!(keys_equal("abc123", "abc123"));
        assert!(!keys_equal("abc123", "abc124"));
        assert!(!keys_equal("abc123", "abc12"));
        assert!(!keys_equal("abc123", "abc1234"));
        assert!(!keys_equal("", "abc123"));
    }
    fn keyed(value: Option<&str>) -> Request {
        let mut headers = std::collections::HashMap::new();
        if let Some(v) = value {
            headers.insert(API_KEY_HEADER.to_string(), v.to_string());
        }
        Request { method: "GET".into(), path: "/api/4/cpu".into(), query: String::new(),
                  version: "HTTP/1.1".into(), headers, body: vec![] }
    }
    #[test]
    fn key_gate_trims_and_rejects_blanks() {
        assert!(key_header_ok(&keyed(Some("s3cret")), "s3cret"));
        assert!(key_header_ok(&keyed(Some("  s3cret  ")), "s3cret"));
        assert!(!key_header_ok(&keyed(Some("wrong")), "s3cret"));
        assert!(!key_header_ok(&keyed(Some("")), "s3cret"));
        assert!(!key_header_ok(&keyed(Some("   ")), "s3cret"));
        assert!(!key_header_ok(&keyed(None), "s3cret"));
    }
    #[test]
    fn gate_matrix() {
        assert!(!gate_applies("/api/4/cpu", false, false));
        assert!(!gate_applies("/favicon.ico", true, true));
        assert!(gate_applies("/", true, false));
        assert!(gate_applies("/api/4/cpu", true, false));
        assert!(!gate_applies("/", false, true));
        assert!(!gate_applies("/dashboard", false, true));
        assert!(gate_applies("/api/4/cpu", false, true));
        assert!(gate_applies("/openapi.json", false, true));
        assert!(gate_applies("/", true, true));
    }
}
