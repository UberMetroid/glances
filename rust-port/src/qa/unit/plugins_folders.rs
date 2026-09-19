//! Unit tests for the folders plugin — recursive size computation.
//!
//! These tests build a small temp directory tree under
//! `std::env::temp_dir()` and verify `dir_size` sums the bytes of all
//! regular files underneath. We don't use a full `tempfile` crate (per
//! the no-crates rule), so each test generates a UUID-like suffix from
//! the test name + process id + nanos and cleans up after itself.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::plugins::folders::{configured_folders, dir_size, MAX_DEPTH};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Build a unique temp directory path.
fn unique_tmp(label: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let pid = std::process::id();
    let p = env::temp_dir().join(format!("glances_rs_test_{}_{}_{}_{}", label, pid, ts, n));
    fs::create_dir_all(&p).unwrap();
    p
}

fn cleanup(p: &Path) {
    let _ = fs::remove_dir_all(p);
}

fn write_file(p: &Path, contents: &[u8]) {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, contents).unwrap();
}

#[test]
fn empty_directory_returns_zero() {
    let d = unique_tmp("empty");
    assert_eq!(dir_size(&d).unwrap(), 0);
    cleanup(&d);
}

#[test]
fn missing_directory_returns_err() {
    let bogus = env::temp_dir().join("glances_rs_does_not_exist_xyz_9999");
    assert!(dir_size(&bogus).is_err());
}

#[test]
fn single_file_size_matches() {
    let d = unique_tmp("single");
    let f = d.join("a.txt");
    write_file(&f, b"hello");
    assert_eq!(dir_size(&f).unwrap(), 5);
    cleanup(&d);
}

#[test]
fn recursive_sum_aggregates_all_files() {
    let d = unique_tmp("recurse");
    // a/1.txt (3), a/b/2.txt (5), a/b/c/3.txt (7), top.txt (1)
    write_file(&d.join("top.txt"), b"x");
    write_file(&d.join("a").join("1.txt"), b"foo");
    write_file(&d.join("a").join("b").join("2.txt"), b"hello");
    write_file(&d.join("a").join("b").join("c").join("3.txt"), b"1234567");
    let total = dir_size(&d).unwrap();
    assert_eq!(total, 1 + 3 + 5 + 7);
    cleanup(&d);
}

#[test]
fn symlinks_are_not_followed() {
    let d = unique_tmp("symlink");
    // Real file with 10 bytes; symlink pointing to it; expect 10, not 20.
    let real = d.join("real.txt");
    write_file(&real, b"0123456789");
    let link = d.join("link.txt");
    // Skip on platforms where symlink fails (e.g. some Windows setups).
    match std::os::unix::fs::symlink(&real, &link) {
        Ok(()) => {
            let total = dir_size(&d).unwrap();
            assert_eq!(total, 10);
        }
        Err(_) => {
            // Cleanup and pass; symlink not supported in this env.
        }
    }
    cleanup(&d);
}

#[test]
fn configured_folders_is_empty_for_now() {
    // M6-followup will wire this into Args / config.
    assert!(configured_folders().is_empty());
}

#[test]
fn max_depth_is_set() {
    // We rely on MAX_DEPTH being > 0 so deep trees don't lock up.
    assert!(MAX_DEPTH >= 4);
}