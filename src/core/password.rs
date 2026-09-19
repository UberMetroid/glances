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

/// One entry in the password file.
#[derive(Debug, Clone)]
pub struct PasswordEntry {
    pub username: String,
    pub hash: PasswordHash,
}

/// Stored password hash. `Plain` is bare sha256 hex; `Salted` is the
/// `$sha256$<salt>$<hash>` form Python uses when a salt is provided
/// (`glances/secure.py:54-72`).
#[derive(Debug, Clone, PartialEq)]
pub enum PasswordHash {
    Plain(String),
    Salted { salt: String, hash: String },
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
        };
        let stored = match self {
            PasswordHash::Plain(h) => h,
            PasswordHash::Salted { hash, .. } => hash,
        };
        hex::const_time_eq(stored.as_bytes(), computed.as_bytes())
    }
}

/// Loaded password file.
pub struct PasswordFile {
    pub entries: HashMap<String, PasswordHash>,
    pub path: PathBuf,
}

impl PasswordFile {
    pub fn empty() -> Self {
        Self { entries: HashMap::new(), path: PathBuf::new() }
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
        Ok(Self { entries, path: path.to_path_buf() })
    }

    /// Default path: `$XDG_CONFIG_HOME/glances/glances.pwd` or OS equivalent.
    pub fn default_path() -> PathBuf { user_dir().join("glances.pwd") }

    /// Load from the default path. Returns empty if missing.
    pub fn load_default() -> Result<Self> { Self::load(&Self::default_path()) }

    /// Verify `password` against the stored hash for `username`.
    /// Runs a hash verification even when the user doesn't exist so the
    /// timing difference can't be used to enumerate valid usernames.
    pub fn check(&self, username: &str, password: &str) -> bool {
        let dummy = PasswordHash::Salted {
            salt: "00".to_string(),
            hash: sha256_hex(b"glances-rs-dummy"),
        };
        let hash = self.entries.get(username).unwrap_or(&dummy);
        let ok = hash.verify(password);
        ok && self.entries.contains_key(username)
    }

    /// Add or replace an entry using a freshly generated 8-byte salt.
    pub fn set(&mut self, username: &str, password: &str) {
        let salt_bytes = generate_salt(8);
        let salt = hex::encode(&salt_bytes);
        let mut buf = Vec::with_capacity(salt_bytes.len() + password.len());
        buf.extend_from_slice(&salt_bytes);
        buf.extend_from_slice(password.as_bytes());
        let hash = sha256_hex(&buf);
        self.entries.insert(username.to_string(), PasswordHash::Salted { salt, hash });
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
    fn password_file_accepts_python_format() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("glances-rs-py-{nanos}"));
        let salt = "deadbeef";
        // Python: sha256(salt_bytes || password_bytes) where salt_bytes is
        // the hex-decoded salt. So hex-decode "deadbeef" → [0xde,0xad,0xbe,0xef]
        // and concat with b"secret".
        let salt_bytes: Vec<u8> = vec![0xde, 0xad, 0xbe, 0xef];
        let mut buf = Vec::new();
        buf.extend_from_slice(&salt_bytes);
        buf.extend_from_slice(b"secret");
        let hash = sha256_hex(&buf);
        let content = format!("# glances password file\nadmin:$sha256${salt}${hash}\nbob:plaindeadbeef\n");
        std::fs::write(&path, content).unwrap();
        let loaded = PasswordFile::load(&path).unwrap();
        assert!(loaded.check("admin", "secret"));
        assert!(!loaded.check("admin", "other"));
        assert!(!loaded.check("bob", "secret"));
        let _ = std::fs::remove_file(path);
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
