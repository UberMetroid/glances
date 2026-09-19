//! Hex encoding/decoding. Std-only (no `hex` crate per AC-1).

/// Lowercase hex of a byte slice.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes { out.push_str(&format!("{:02x}", b)); }
    out
}

/// Decode lowercase or uppercase hex into bytes. Returns `None` on bad input.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 { return None; }
    let mut out = Vec::with_capacity(s.len() / 2);
    let chars: Vec<u8> = s.bytes().collect();
    let mut i = 0;
    while i < chars.len() {
        let hi = nibble(chars[i])?;
        let lo = nibble(chars[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Constant-time equality. Used to thwart timing attacks on hash compares.
pub fn const_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) { diff |= x ^ y; }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encode_basic() {
        assert_eq!(encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(encode(&[]), "");
        assert_eq!(encode(&[0x00]), "00");
        assert_eq!(encode(&[0xff, 0xff]), "ffff");
    }
    #[test]
    fn decode_basic() {
        assert_eq!(decode("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(decode("DEADBEEF"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(decode(""), Some(vec![]));
        assert_eq!(decode("xyz"), None);
        assert_eq!(decode("abc"), None); // odd length
    }
    #[test]
    fn roundtrip() {
        for n in 0..50 {
            let bytes: Vec<u8> = (0..n).map(|i| (i * 7 + 13) as u8).collect();
            let encoded = encode(&bytes);
            let decoded = decode(&encoded).unwrap();
            assert_eq!(bytes, decoded);
        }
    }
    #[test]
    fn const_time_eq_works() {
        assert!(const_time_eq(b"hello", b"hello"));
        assert!(!const_time_eq(b"hello", b"world"));
        assert!(!const_time_eq(b"hello", b"hell"));
    }
}
