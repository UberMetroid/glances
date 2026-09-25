//! Test harness utilities. Only built under `#[cfg(test)]`.

use std::path::PathBuf;

/// Serializes tests that mutate process-global env (`GLANCES_HELPER_DIR`).
/// Unit tests run on threads; the env var is process-wide, so every test
/// that sets it must hold this lock from set through remove.
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// RAII `GLANCES_HELPER_DIR` scope: sets the helper override on build,
/// removes it on drop (even on assert panic), holding `ENV_LOCK` for
/// the whole scope so parallel tests never observe a foreign dir.
pub struct HelperEnv {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl HelperEnv {
    pub fn set(dir: &std::path::Path) -> Self {
        // Poison-tolerant: a panicking holder still runs Drop (which
        // removes the var), so the invariant holds and the next test
        // may proceed — one failure must not cascade into all.
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: every env mutation site in the test binary goes
        // through this guard, and the lock is held for the whole
        // scope — no concurrent set/remove/getenv race is possible.
        unsafe { std::env::set_var("GLANCES_HELPER_DIR", dir) };
        Self { _lock: lock }
    }
}

impl Drop for HelperEnv {
    fn drop(&mut self) {
        // SAFETY: same lock still held; see `set`.
        unsafe { std::env::remove_var("GLANCES_HELPER_DIR") };
    }
}

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
