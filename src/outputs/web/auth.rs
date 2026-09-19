//! HTTP Basic auth — header parsing + PasswordFile verification.
//!
//! Mirrors the Python Glances `glances/auth.py` flow: parse the
//! `Authorization: Basic base64(user:pass)` header, look up `user` in
//! the password file, and verify the SHA-256 hash (plain or salted).
//!
//! We deliberately use `PasswordFile::check` (already in core/) for the
//! crypto side. This module just bridges HTTP ↔ PasswordFile.

use crate::core::hex;
use crate::core::password::PasswordFile;

const BASIC_PREFIX: &str = "Basic ";

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

/// Verify using the hex crate's constant-time compare as a defense-in-depth
/// check on the username lookup. (Password comparison is already constant-
/// time via `PasswordHash::verify`.)
#[allow(dead_code)]
pub fn verify_constant_time(pw: &PasswordFile, user: &str, pass: &str) -> bool {
    // Lookup is O(n) and the keys are short; the timing surface here is
    // the length of `user`, not its content. Constant-time compare on the
    // user string is therefore a defense-in-depth measure, not a primary
    // defense.
    let mut found: Option<&str> = None;
    for key in pw.entries.keys() {
        if hex::const_time_eq(key.as_bytes(), user.as_bytes()) {
            found = Some(key.as_str());
            break;
        }
    }
    match found {
        None => false,
        Some(k) => pw.check(k, pass),
    }
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
}
