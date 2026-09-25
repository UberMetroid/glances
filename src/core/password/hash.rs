//! Stored password hashes (legacy SHA-256 + Python-compatible PBKDF2).

use super::super::hex;
use super::super::pbkdf2::glances_pbkdf2;
use super::super::sha256::sha256_hex;

/// Stored password hash:
/// - `Plain`: bare sha256 hex (legacy local form).
/// - `Salted`: our `$sha256$<salt>$<hash>` form (single SHA-256 over
///   hex-decoded salt + password).
/// - `Pbkdf2`: Python form `salt$hex`, where hex =
///   `pbkdf2_hmac('sha256', password, salt, 100000, dklen=128).hex()`
///   and salt is the raw salt *string* (`salt.encode()`, NOT decoded).
#[derive(Debug, Clone, PartialEq)]
pub enum PasswordHash {
    Plain(String),
    Salted { salt: String, hash: String },
    Pbkdf2 { salt: String, hash: String },
}

impl PasswordHash {
    pub fn verify(&self, password: &str) -> bool {
        let computed = match self {
            PasswordHash::Plain(_) => sha256_hex(password.as_bytes()),
            PasswordHash::Salted { salt, .. } => {
                let salt_bytes = hex::decode(salt).unwrap_or_default();
                let mut buf = Vec::with_capacity(salt_bytes.len() + password.len());
                buf.extend_from_slice(&salt_bytes);
                buf.extend_from_slice(password.as_bytes());
                sha256_hex(&buf)
            }
            PasswordHash::Pbkdf2 { salt, .. } => glances_pbkdf2(password.as_bytes(), salt),
        };
        let stored = match self {
            PasswordHash::Plain(h) => h,
            PasswordHash::Salted { hash, .. } => hash,
            PasswordHash::Pbkdf2 { hash, .. } => hash,
        };
        hex::const_time_eq(stored.as_bytes(), computed.as_bytes())
    }
}

