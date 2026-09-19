//! Verifies --help / --version don't crash and produce expected substrings.
//!
//! These tests run inside `src/qa/integration/` and the env var
//! `CARGO_BIN_EXE_glances-rs` is only set for `tests/` integration tests.
//! We resolve the binary relative to `CARGO_MANIFEST_DIR` + `target/debug/`.

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
fn version_flag_prints_version() {
    let out = Command::new(glances_bin())
        .arg("--version")
        .output()
        .expect("binary should run");
    assert!(out.status.success(), "exit code {:?}", out.status.code());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("glances-rs"), "version output: {}", s);
}

#[test]
fn help_flag_prints_help() {
    let out = Command::new(glances_bin())
        .arg("--help")
        .output()
        .expect("binary should run");
    assert!(out.status.success(), "exit code {:?}", out.status.code());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("--webserver"), "help missing key flags: {}", s);
    assert!(s.contains("--client"), "help missing key flags: {}", s);
}
