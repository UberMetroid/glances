//! Test harness utilities. Only built under `#[cfg(test)]`.

use std::path::PathBuf;

/// RAII tempdir — deletes the directory on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("glances-rs-{}-{}-{}-{}", label, std::process::id(), n, seq));
        // create_dir (not _all): fails on collision instead of silently
        // sharing a dir between two tests.
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    pub fn path(&self) -> &std::path::Path { &self.0 }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
