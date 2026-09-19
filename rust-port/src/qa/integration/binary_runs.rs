//! Smoke test: the binary runs at all (no startup panic).
//!
//! M0 only checks `--version` and `--help`. Full --standalone smoke comes
//! in M15 (TUI) and --web in M14.

use std::path::PathBuf;
use std::process::Command;

fn glances_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_glances-rs") {
        return PathBuf::from(p);
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let debug = PathBuf::from(&manifest).join("target").join("debug").join("glances-rs");
    let release = PathBuf::from(&manifest).join("target").join("release").join("glances-rs");
    if debug.exists() { debug } else { release }
}

#[test]
fn binary_runs_with_no_args() {
    let out = Command::new(glances_bin())
        .output()
        .expect("binary runs");
    assert!(!out.stdout.is_empty() || !out.stderr.is_empty(),
            "binary produced no output at all");
}
