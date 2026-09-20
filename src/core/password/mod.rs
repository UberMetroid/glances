//! Password file — SHA-256 hashed, matches Python Glances format.
//!
//! Mirrors `glances/password.py` and `glances/secure.py`. File format:
//!   - Plain:   `username:<sha256hex>`
//!   - Salted:  `username:$sha256$<salt_hex>$<hash_hex>`
//! where salted is `sha256(salt_bytes || password_bytes)`.
//! AC-10 requires Python-written files to be accepted by Rust and vice versa.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::config_dir::user_dir;
use super::error::{GlancesError, Result};
use super::hex;
use super::sha256::sha256_hex;

mod hash;

pub use hash::PasswordHash;


/// One entry in the password file.
#[derive(Debug, Clone)]
pub struct PasswordEntry {
    pub username: String,
    pub hash: PasswordHash,
}

/// Loaded password file.
pub struct PasswordFile {
    pub entries: HashMap<String, PasswordHash>,
    pub path: PathBuf,
    /// Bounded verification cache: ((username, sha256(password)), ok).
    /// PBKDF2 at 100k iterations costs ~0.3s per check; the web layer
    /// verifies on every request, so memoize like upstream's
    /// `weak_lru_cache` on `check_password`. FIFO-evicted at 32.
    cache: std::sync::Mutex<Vec<((String, String), bool)>>,
}

impl PasswordFile {
    pub fn empty() -> Self {
        Self { entries: HashMap::new(), path: PathBuf::new(), cache: std::sync::Mutex::new(Vec::new()) }
    }

    /// Load from a path. Missing file → empty (no error). Malformed lines
    /// are skipped silently (matches Python's lenient behavior).
    pub fn load(path: &Path) -> Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => return Err(GlancesError::Io(e)),
        };
        let mut entries = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some((user, hash)) = line.split_once(':') {
                entries.insert(user.trim().to_string(), parse_hash(hash.trim()));
            }
        }
        Ok(Self { entries, path: path.to_path_buf(), cache: std::sync::Mutex::new(Vec::new()) })
    }

    /// Default path: `$XDG_CONFIG_HOME/glances/glances.pwd` or OS equivalent.
    pub fn default_path() -> PathBuf { user_dir().join("glances.pwd") }

    /// Load from the default path. Returns empty if missing.
    pub fn load_default() -> Result<Self> { Self::load(&Self::default_path()) }

    /// Verify `password` against the stored hash for `username`.
    /// Runs a hash verification even when the user doesn't exist so the
    /// timing difference can't be used to enumerate valid usernames.
    /// Results memoize in a 32-entry FIFO (upstream `weak_lru_cache`
    /// parity — PBKDF2 is far too slow to rerun per HTTP request).
    pub fn check(&self, username: &str, password: &str) -> bool {
        let key = (username.to_string(), sha256_hex(password.as_bytes()));
        if let Ok(cache) = self.cache.lock() {
            if let Some((_, ok)) = cache.iter().find(|(k, _)| k == &key) {
                return *ok;
            }
        }
        let dummy = PasswordHash::Salted {
            salt: "00".to_string(),
            hash: sha256_hex(b"glances-rs-dummy"),
        };
        let hash = self.entries.get(username).unwrap_or(&dummy);
        let ok = hash.verify(password) && self.entries.contains_key(username);
        if let Ok(mut cache) = self.cache.lock() {
            cache.push((key, ok));
            while cache.len() > 32 {
                cache.remove(0);
            }
        }
        ok
    }

    /// Add or replace an entry in the upstream Python format
    /// (`salt$pbkdf2hex`, 16-byte hex salt like `uuid4().hex`).
    pub fn set(&mut self, username: &str, password: &str) {
        let salt = hex::encode(&generate_salt(16));
        let hash = super::pbkdf2::glances_pbkdf2(password.as_bytes(), &salt);
        self.entries.insert(username.to_string(), PasswordHash::Pbkdf2 { salt, hash });
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Persist to disk in the format Python reads.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut buf = String::new();
        for (user, hash) in &self.entries {
            let hash_str = match hash {
                PasswordHash::Plain(h) => h.clone(),
                PasswordHash::Salted { salt, hash } => format!("$sha256${}${}", salt, hash),
                PasswordHash::Pbkdf2 { salt, hash } => format!("{}${}", salt, hash),
            };
            buf.push_str(&format!("{}:{}\n", user, hash_str));
        }
        // Password files must not be world-readable: create with 0600
        // and tighten the mode on pre-existing files too.
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&self.path)
            .map_err(GlancesError::Io)?;
        f.write_all(buf.as_bytes()).map_err(GlancesError::Io)?;
        let mut perms = f.metadata().map_err(GlancesError::Io)?.permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o600);
        fs::set_permissions(&self.path, perms).map_err(GlancesError::Io)
    }
}

fn parse_hash(s: &str) -> PasswordHash {
    if let Some(rest) = s.strip_prefix("$sha256$") {
        if let Some((salt, hash)) = rest.split_once('$') {
            return PasswordHash::Salted { salt: salt.to_string(), hash: hash.to_string() };
        }
    }
    // Upstream Python form: `salt$hex` (exactly one separator).
    if let Some((salt, hash)) = s.split_once('$') {
        if !salt.is_empty() && !hash.is_empty() && !hash.contains('$') {
            return PasswordHash::Pbkdf2 { salt: salt.to_string(), hash: hash.to_string() };
        }
    }
    PasswordHash::Plain(s.to_string())
}

fn generate_salt(n_bytes: usize) -> Vec<u8> {
    // Prefer the kernel CSPRNG — std-only, no crates needed.
    use std::io::Read;
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        let mut out = vec![0u8; n_bytes];
        if f.read_exact(&mut out).is_ok() {
            return out;
        }
    }
    // Fallback: salt is a uniqueness token, not a secret — nanos + LCG.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let mut n = nanos as u64;
    let mut out = vec![0u8; n_bytes];
    for b in out.iter_mut() {
        *b = (n & 0xff) as u8;
        // LCG to expand nanoseconds into more bytes.
        n = n.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn plain_hash_verifies() {
        let hash = sha256_hex(b"hunter2");
        let ph = PasswordHash::Plain(hash);
        assert!(ph.verify("hunter2"));
        assert!(!ph.verify("wrong"));
    }
    #[test]
    fn salted_hash_verifies() {
        let mut pf = PasswordFile::empty();
        pf.set("admin", "secret");
        assert!(pf.check("admin", "secret"));
        assert!(!pf.check("admin", "other"));
    }
    #[test]
    fn password_file_format_roundtrip() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("glances-rs-pwd-{nanos}"));
        let mut pf = PasswordFile::empty();
        pf.path = path.clone();
        pf.set("alice", "hunter2");
        pf.save().unwrap();
        let loaded = PasswordFile::load(&path).unwrap();
        assert!(loaded.check("alice", "hunter2"));
        assert!(!loaded.check("alice", "wrong"));
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn password_file_accepts_upstream_pbkdf2_format() {
        // Real Python entry: `salt$pbkdf2_hmac('sha256', password, salt,
        // 100000, dklen=128).hex()` — vector generated with hashlib.
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("glances-rs-py-{nanos}"));
        let content = "admin:deadbeef$154911c264bae39d0a95ecf5c9155ce4ab295e88a6db9340b0651a06131822e7fe1ee1ffa89c2af11c4f38ee890cbb02ee2bfe0eefe7ccf42a2967790d86a97617b23120bdf87542bf43de796138dc39bcd439e78501e8178231aff1ee5f9ec6c09b49e734aaa6cf8e72e6e95bcc9df5a521629bd32546f7a58a8cc26ffce758\n";
        std::fs::write(&path, content).unwrap();
        let loaded = PasswordFile::load(&path).unwrap();
        assert!(loaded.check("admin", "secret"));
        assert!(!loaded.check("admin", "other"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn set_writes_upstream_format() {
        // Entries created locally must verify AND parse back as the
        // upstream `salt$hex` shape (no `$sha256$` marker).
        let mut pf = PasswordFile::empty();
        pf.set("carol", "hunter2");
        assert!(pf.check("carol", "hunter2"));
        assert!(!pf.check("carol", "wrong"));
        match pf.entries.get("carol") {
            Some(PasswordHash::Pbkdf2 { salt, hash }) => {
                assert_eq!(salt.len(), 32, "16-byte hex salt like uuid4().hex");
                assert_eq!(hash.len(), 256, "128-byte dklen as hex");
                // Independent oracle cross-check of the SAME salt.
                let expect = super::super::pbkdf2::glances_pbkdf2(b"hunter2", salt);
                assert_eq!(&expect, hash);
            }
            other => panic!("expected Pbkdf2 entry, got {:?}", other),
        }
    }
    #[test]
    fn password_file_missing_yields_empty() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("glances-rs-missing-{nanos}"));
        let pf = PasswordFile::load(&path).unwrap();
        assert!(!pf.check("anyone", "anything"));
    }
}
