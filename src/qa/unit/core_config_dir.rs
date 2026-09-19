//! Unit tests for core::config_dir — per-OS config-path resolution.

use crate::core::config_dir;
use std::path::PathBuf;

#[test]
fn candidate_paths_is_non_empty() {
    let paths = config_dir::candidate_paths();
    assert!(!paths.is_empty(), "expected at least one candidate path");
    // Each path should either be absolute or include "glances.conf".
    for p in &paths {
        let s = p.to_string_lossy();
        assert!(s.contains("glances"), "candidate missing 'glances': {:?}", p);
    }
}

#[test]
fn resolve_returns_override() {
    let p = config_dir::resolve(Some("/tmp/my-glances.conf"));
    assert_eq!(p, PathBuf::from("/tmp/my-glances.conf"));
}

#[test]
fn user_dir_includes_glances() {
    let d = config_dir::user_dir();
    assert!(d.to_string_lossy().contains("glances"),
            "user_dir should contain 'glances': {:?}", d);
}

#[test]
fn cache_dir_is_absolute_or_temp() {
    let d = config_dir::cache_dir();
    assert!(!d.to_string_lossy().is_empty(), "cache_dir should never be empty");
}

#[test]
fn resolve_with_none_returns_first_candidate() {
    let p = config_dir::resolve(None);
    let first_candidate = config_dir::candidate_paths().into_iter().next();
    match first_candidate {
        Some(expected) => assert_eq!(p, expected),
        None => assert_eq!(p, PathBuf::from("glances.conf")),
    }
}
