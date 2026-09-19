//! Test harness utilities. Only built under `#[cfg(test)]`.

use std::path::PathBuf;

/// RAII tempdir — deletes the directory on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        let mut p = std::env::temp_dir();
        let n: u64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        p.push(format!("glances-rs-{}-{}", label, n));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    pub fn path(&self) -> &std::path::Path { &self.0 }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Capture stdout writes inside a closure (for `println!` testing).
pub fn capture_stdout<F: FnOnce()>(f: F) -> String {
    // Use a pipe + fork dance? Without `nix`/`ioctl` crates we can only do
    // a poor man's version. For M0 this is a stub returning empty string.
    // Real impl lands when we have a TTY-free testing helper.
    f();
    String::new()
}
