//! PBKDF2-HMAC-SHA256 over the in-tree SHA-256, no caching.
//!
//! Callers cache at their own layer (verification runs per HTTP
//! request). Iteration count and key length are explicit parameters so
//! tests exercise cheap vectors; production uses the file format's
//! parameters below.

use super::sha256::sha256;

/// HMAC-SHA256 with the standard 64-byte block (long keys hash first).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        block[..32].copy_from_slice(&sha256(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + msg.len());
    let mut outer = Vec::with_capacity(64 + 32);
    inner.extend(block.iter().map(|b| b ^ 0x36));
    outer.extend(block.iter().map(|b| b ^ 0x5c));
    inner.extend_from_slice(msg);
    let mid = sha256(&inner);
    outer.extend_from_slice(&mid);
    sha256(&outer)
}

/// PBKDF2-HMAC-SHA256: xor-accumulated HMAC chains per 32-byte block,
/// truncated to the requested key length.
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, dklen: usize) -> Vec<u8> {
    let mut key = Vec::with_capacity(dklen.div_ceil(32) * 32);
    for block_no in 1..=dklen.div_ceil(32) as u32 {
        let mut seed = Vec::with_capacity(salt.len() + 4);
        seed.extend_from_slice(salt);
        seed.extend_from_slice(&block_no.to_be_bytes());
        let mut u = hmac_sha256(password, &seed);
        let mut acc = u;
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for (a, b) in acc.iter_mut().zip(u.iter()) {
                *a ^= *b;
            }
        }
        key.extend_from_slice(&acc);
    }
    key.truncate(dklen);
    key
}

/// The credential file's parameters: 100 000 iterations, 128-byte key.
pub const GLANCES_ITERATIONS: u32 = 100_000;
pub const GLANCES_DKLEN: usize = 128;

/// Production hash as 256 lowercase hex chars. The salt is the raw
/// salt *string's* bytes — never hex-decoded.
pub fn glances_pbkdf2(password: &[u8], salt: &str) -> String {
    let dk = pbkdf2_hmac_sha256(password, salt.as_bytes(), GLANCES_ITERATIONS, GLANCES_DKLEN);
    dk.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    #[test]
    fn rfc_vector_single_iteration() {
        // PBKDF2-HMAC-SHA256("password","salt",1,32) per RFC 7914 §11.
        assert_eq!(
            hex(&pbkdf2_hmac_sha256(b"password", b"salt", 1, 32)),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
    }
    #[test]
    fn production_params_match_hashlib() {
        // hashlib.pbkdf2_hmac('sha256', b'secret', b'deadbeef',
        // 100000, dklen=128).hex()
        assert_eq!(
            glances_pbkdf2(b"secret", "deadbeef"),
            "154911c264bae39d0a95ecf5c9155ce4ab295e88a6db9340b0651a06131822e7fe1ee1ffa89c2af11c4f38ee890cbb02ee2bfe0eefe7ccf42a2967790d86a97617b23120bdf87542bf43de796138dc39bcd439e78501e8178231aff1ee5f9ec6c09b49e734aaa6cf8e72e6e95bcc9df5a521629bd32546f7a58a8cc26ffce758"
        );
    }
    #[test]
    fn output_length_tracks_dklen() {
        assert!(pbkdf2_hmac_sha256(b"p", b"s", 1, 0).is_empty());
        assert_eq!(pbkdf2_hmac_sha256(b"p", b"s", 1, 33).len(), 33);
    }
}
