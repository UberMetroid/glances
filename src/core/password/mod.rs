//! Credential file: load, verify, and persist logins.
//!
//! Three hash shapes share the `user:hash` line format (frozen — the
//! Python tool reads and writes this same file):
//! - plain sha256 hex: `user:<hex>`
//! - salted sha256: `user:$sha256$<salt>$<hex>` over decoded salt bytes + password
//! - PBKDF2: `user:<salt>$<hex>` with exactly one separator

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::config_dir::user_dir;
use super::error::{GlancesError, Result};
use super::hex;
use super::sha256::sha256_hex;

mod hash;
mod prompt;

pub use hash::PasswordHash;
pub use prompt::{resolve_auth, resolve_mode_auth};

/// One parsed credential line.
#[derive(Debug, Clone)]
pub struct PasswordEntry {
    pub username: String,
    pub hash: PasswordHash,
}

/// Loaded credential file with a bounded verification cache (PBKDF2 is
/// far too slow to rerun per HTTP request): (user, sha256(password))
/// → verdict, FIFO-evicted at 32 entries.
pub struct PasswordFile {
    pub entries: HashMap<String, PasswordHash>,
    pub path: PathBuf,
    cache: std::sync::Mutex<Vec<((String, String), bool)>>,
}

impl PasswordFile {
    pub fn empty() -> Self {
        Self { entries: HashMap::new(), path: PathBuf::new(), cache: std::sync::Mutex::new(Vec::new()) }
    }

    /// Load from disk. A missing file loads empty; blank lines,
    /// `#` comments, and colon-less lines are skipped.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => return Err(GlancesError::Io(e)),
        };
        let mut entries = HashMap::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((user, hash)) = line.split_once(':') {
                entries.insert(user.trim().to_string(), parse_hash(hash.trim()));
            }
        }
        Ok(Self { entries, path: path.to_path_buf(), cache: std::sync::Mutex::new(Vec::new()) })
    }

    pub fn default_path() -> PathBuf { user_dir().join("glances.pwd") }

    pub fn load_default() -> Result<Self> { Self::load(&Self::default_path()) }

    /// Verify a password. Unknown users still pay for one hash run so
    /// timing can't enumerate valid names. Verdicts memoize.
    pub fn check(&self, username: &str, password: &str) -> bool {
        let key = (username.to_string(), sha256_hex(password.as_bytes()));
        if let Ok(cache) = self.cache.lock()
            && let Some((_, ok)) = cache.iter().find(|(k, _)| k == &key) {
                return *ok;
            }
        let dummy = PasswordHash::Salted {
            salt: "00".to_string(),
            hash: sha256_hex(b"glances-rs-dummy"),
        };
        let stored = self.entries.get(username).unwrap_or(&dummy);
        let ok = stored.verify(password) && self.entries.contains_key(username);
        if let Ok(mut cache) = self.cache.lock() {
            cache.push((key, ok));
            while cache.len() > 32 {
                cache.remove(0);
            }
        }
        ok
    }

    /// Set (or replace) a credential with a fresh 16-byte hex salt and a
    /// PBKDF2 hash. Clears the verification cache.
    pub fn set(&mut self, username: &str, password: &str) {
        let salt = hex::encode(&fresh_salt(16));
        let hash = super::pbkdf2::glances_pbkdf2(password.as_bytes(), &salt);
        self.entries.insert(username.to_string(), PasswordHash::Pbkdf2 { salt, hash });
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Write every entry back in `user:hash` form, creating parent dirs.
    /// The file is created 0600 and tightened to 0600 when it already
    /// exists — credentials are never world-readable.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut buf = String::new();
        for (user, hash) in &self.entries {
            let rendered = match hash {
                PasswordHash::Plain(h) => h.clone(),
                PasswordHash::Salted { salt, hash } => format!("$sha256${salt}${hash}"),
                PasswordHash::Pbkdf2 { salt, hash } => format!("{salt}${hash}"),
            };
            buf.push_str(&format!("{user}:{rendered}\n"));
        }
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&self.path)
            .map_err(GlancesError::Io)?;
        f.write_all(buf.as_bytes()).map_err(GlancesError::Io)?;
        let mut perms = f.metadata().map_err(GlancesError::Io)?.permissions();
        use std::os::unix::fs::PermissionsExt as _;
        perms.set_mode(0o600);
        fs::set_permissions(&self.path, perms).map_err(GlancesError::Io)
    }
}

fn parse_hash(s: &str) -> PasswordHash {
    if let Some(rest) = s.strip_prefix("$sha256$")
        && let Some((salt, hash)) = rest.split_once('$') {
            return PasswordHash::Salted { salt: salt.to_string(), hash: hash.to_string() };
        }
    if let Some((salt, hash)) = s.split_once('$')
        && !salt.is_empty() && !hash.is_empty() && !hash.contains('$') {
            return PasswordHash::Pbkdf2 { salt: salt.to_string(), hash: hash.to_string() };
        }
    PasswordHash::Plain(s.to_string())
}

/// Uniqueness bytes: kernel CSPRNG first, time-seeded stream fallback
/// (a salt is a uniqueness token, not a secret).
fn fresh_salt(n_bytes: usize) -> Vec<u8> {
    use std::io::Read as _;
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        let mut out = vec![0u8; n_bytes];
        if f.read_exact(&mut out).is_ok() {
            return out;
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut state = nanos as u64;
    let mut out = vec![0u8; n_bytes];
    for b in out.iter_mut() {
        *b = (state & 0xff) as u8;
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(stem: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let mut p = std::env::temp_dir();
        p.push(format!("glances-rs-{stem}-{nanos}"));
        p
    }

    #[test]
    fn plain_and_salted_shapes_verify() {
        let plain = PasswordHash::Plain(sha256_hex(b"hunter2"));
        assert!(plain.verify("hunter2"));
        assert!(!plain.verify("wrong"));
        let mut pf = PasswordFile::empty();
        pf.set("admin", "secret");
        assert!(pf.check("admin", "secret"));
        assert!(!pf.check("admin", "other"));
    }

    #[test]
    fn save_load_roundtrip_keeps_credentials() {
        let path = scratch("pwd");
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
    fn python_pbkdf2_entry_verifies() {
        // Real hashlib entry: salt$pbkdf2_hmac('sha256', password,
        // salt, 100000, dklen=128).hex().
        let path = scratch("py");
        let content = "admin:deadbeef$154911c264bae39d0a95ecf5c9155ce4ab295e88a6db9340b0651a06131822e7fe1ee1ffa89c2af11c4f38ee890cbb02ee2bfe0eefe7ccf42a2967790d86a97617b23120bdf87542bf43de796138dc39bcd439e78501e8178231aff1ee5f9ec6c09b49e734aaa6cf8e72e6e95bcc9df5a521629bd32546f7a58a8cc26ffce758\n";
        std::fs::write(&path, content).unwrap();
        let loaded = PasswordFile::load(&path).unwrap();
        assert!(loaded.check("admin", "secret"));
        assert!(!loaded.check("admin", "other"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn local_entries_use_salt_dollar_hex_shape() {
        let mut pf = PasswordFile::empty();
        pf.set("carol", "hunter2");
        assert!(pf.check("carol", "hunter2"));
        assert!(!pf.check("carol", "wrong"));
        match pf.entries.get("carol") {
            Some(PasswordHash::Pbkdf2 { salt, hash }) => {
                assert_eq!(salt.len(), 32);
                assert_eq!(hash.len(), 256);
                assert_eq!(&super::super::pbkdf2::glances_pbkdf2(b"hunter2", salt), hash);
            }
            other => panic!("expected Pbkdf2 entry, got {other:?}"),
        }
    }

    #[test]
    fn missing_file_checks_nothing() {
        let pf = PasswordFile::load(&scratch("missing")).unwrap();
        assert!(!pf.check("anyone", "anything"));
    }
}
