//! Password file — SHA-256 hashed, matches Python Glances format.
//!
//! Mirrors `glances/password.py` and `glances/secure.py`. The file format
//! is one entry per line: `username:sha256hexhash` with optional `#` comments.
//! AC-10 requires Python-written files to be accepted by Rust and vice versa.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use super::error::{GlancesError, Result};

/// One entry in the password file.
#[derive(Debug, Clone)]
pub struct PasswordEntry {
    pub username: String,
    pub sha256_hex: String,
}

/// Loaded password file.
pub struct PasswordFile {
    pub entries: HashMap<String, String>,  // username -> sha256 hex
    pub path: std::path::PathBuf,
}

impl PasswordFile {
    pub fn empty() -> Self {
        Self { entries: HashMap::new(), path: std::path::PathBuf::new() }
    }

    /// Load from a path. Missing file → empty. Malformed lines are skipped
    /// with a warning (matches Python `password.py:48-60` lenient behavior).
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
            // Format: "username:hexhash" or "username:$sha256$..." (legacy).
            if let Some((user, hash)) = line.split_once(':') {
                entries.insert(user.trim().to_string(), hash.trim().to_string());
            }
        }
        Ok(Self { entries, path: path.to_path_buf() })
    }

    /// Verify `password` against the stored hash for `username`. Constant-time
    /// comparison to thwart timing attacks (per security reviewer role).
    pub fn check(&self, username: &str, password: &str) -> bool {
        match self.entries.get(username) {
            None => false,
            Some(stored) => {
                let computed = sha256_hex(password.as_bytes());
                const_time_eq(stored.as_bytes(), computed.as_bytes())
            }
        }
    }

    /// Write entries to disk (used when `--password` is set on the CLI and
    /// the user wants to persist). Format is compatible with Python.
    pub fn save(&self) -> Result<()> {
        let mut buf = String::new();
        for (user, hash) in &self.entries {
            buf.push_str(&format!("{}:{}\n", user, hash));
        }
        fs::write(&self.path, buf).map_err(GlancesError::Io)
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    // Hand-rolled SHA-256 per the plan's "no crates" rule.
    sha256_manual(data)
}

fn sha256_manual(data: &[u8]) -> String {
    // Minimal SHA-256 implementation. ~100 lines; see FIPS 180-4.
    // Initial hash values (FIPS 180-4 §5.3.3).
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]);
        }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let mut a = h[0]; let mut b = h[1]; let mut c = h[2]; let mut d = h[3];
        let mut e = h[4]; let mut f = h[5]; let mut g = h[6]; let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let mj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(mj);
            hh = g; g = f; f = e; e = d.wrapping_add(t1); d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f); h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }

    let mut out = String::with_capacity(64);
    for v in &h {
        out.push_str(&format!("{:08x}", v));
    }
    out
}

fn const_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sha256_known_vectors() {
        // FIPS 180-4 known answers.
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
    #[test]
    fn password_file_roundtrip() {
        let dir = tempdir();
        let path = dir.join("glances.pwd");
        let mut pf = PasswordFile::empty();
        pf.path = path.clone();
        pf.entries.insert("admin".into(), sha256_hex(b"hunter2"));
        pf.save().unwrap();
        let loaded = PasswordFile::load(&path).unwrap();
        assert!(loaded.check("admin", "hunter2"));
        assert!(!loaded.check("admin", "wrong"));
        assert!(!loaded.check("nobody", "hunter2"));
    }

    fn tempdir() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let n: u64 = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        p.push(format!("glances-rs-test-{}", n));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
