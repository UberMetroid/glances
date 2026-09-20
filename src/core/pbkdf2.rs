//! PBKDF2-HMAC-SHA256, std-only (upstream `get_hash` parity:
//! `pbkdf2_hmac('sha256', password, salt, 100_000, dklen=128)`).
//!
//! Built on `core::sha256`. Deliberately no caching here — callers
//! (password verification per HTTP request) cache at their layer.

use super::sha256::sha256;

/// HMAC-SHA256 (RFC 2104, block size 64).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut kb = [0u8; 64];
    if key.len() > 64 {
        kb[..32].copy_from_slice(&sha256(key));
    } else {
        kb[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= kb[i];
        opad[i] ^= kb[i];
    }
    let mut inner = Vec::with_capacity(64 + msg.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(msg);
    let ih = sha256(&inner);
    let mut outer = Vec::with_capacity(64 + 32);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&ih);
    sha256(&outer)
}

/// PBKDF2-HMAC-SHA256 (RFC 2898). `iterations` and `dklen` are explicit
/// so tests can run cheap vectors; production uses
/// [`glances_pbkdf2`] (100 000 iterations, 128-byte key — upstream).
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, dklen: usize) -> Vec<u8> {
    let blocks = dklen.div_ceil(32);
    let mut dk = Vec::with_capacity(blocks * 32);
    for block in 1..=blocks as u32 {
        let mut u = {
            let mut sb = Vec::with_capacity(salt.len() + 4);
            sb.extend_from_slice(salt);
            sb.extend_from_slice(&block.to_be_bytes());
            hmac_sha256(password, &sb)
        };
        let mut t = u;
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for (ti, ui) in t.iter_mut().zip(u.iter()) {
                *ti ^= *ui;
            }
        }
        dk.extend_from_slice(&t);
    }
    dk.truncate(dklen);
    dk
}

/// Upstream `GlancesPassword.get_hash` parameters.
pub const GLANCES_ITERATIONS: u32 = 100_000;
pub const GLANCES_DKLEN: usize = 128;

/// `hashlib.pbkdf2_hmac('sha256', password, salt, 100000, dklen=128)`
/// as lowercase hex (256 chars). NOTE: `salt` is the raw salt *string*
/// bytes (`salt.encode()` upstream) — not hex-decoded.
pub fn glances_pbkdf2(password: &[u8], salt: &str) -> String {
    let dk = pbkdf2_hmac_sha256(password, salt.as_bytes(), GLANCES_ITERATIONS, GLANCES_DKLEN);
    let mut out = String::with_capacity(dk.len() * 2);
    for b in &dk {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_7914_aes_vector_short() {
        // PBKDF2-HMAC-SHA256("password", "salt", 1, 32) — RFC 7914 §11
        // test vector (first case).
        let dk = pbkdf2_hmac_sha256(b"password", b"salt", 1, 32);
        let hex: String = dk.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
    }

    #[test]
    fn matches_python_hashlib() {
        // Oracle: hashlib.pbkdf2_hmac('sha256', b'secret',
        // b'deadbeef', 100000, dklen=128).hex()
        assert_eq!(
            glances_pbkdf2(b"secret", "deadbeef"),
            "154911c264bae39d0a95ecf5c9155ce4ab295e88a6db9340b0651a06131822e7fe1ee1ffa89c2af11c4f38ee890cbb02ee2bfe0eefe7ccf42a2967790d86a97617b23120bdf87542bf43de796138dc39bcd439e78501e8178231aff1ee5f9ec6c09b49e734aaa6cf8e72e6e95bcc9df5a521629bd32546f7a58a8cc26ffce758"
        );
    }
}
