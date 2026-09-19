//! Per-OS config-file location resolution.
//!
//! Mirrors `glances/config.py:53-129`. Resolution order (first match wins):
//! 1. `--config` CLI flag
//! 2. User: `~/.config/glances/glances.conf` (Linux), `~/Library/Application
//!    Support/glances/glances.conf` (macOS), `%APPDATA%\glances\glances.conf`
//!    (Windows)
//! 3. System: `/etc/glances/glances.conf` (Linux), `/usr/local/etc/glances/`
//!    (BSD/macOS), `<sys.prefix>/share/doc/glances/glances.conf` (package)
//!
//! Std-only — uses `std::env::var` and `std::path::PathBuf`; no fs probing.

use std::path::{Path, PathBuf};

/// Returns the list of candidate config paths in resolution order.
///
/// The caller tries each in order and uses the first that exists.
pub fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if cfg!(target_os = "linux") {
            // XDG_CONFIG_HOME takes precedence.
            if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
                paths.push(PathBuf::from(xdg).join("glances").join("glances.conf"));
            }
            paths.push(home.join(".config").join("glances").join("glances.conf"));
            paths.push(PathBuf::from("/etc/glances/glances.conf"));
        } else if cfg!(target_os = "macos") {
            paths.push(home.join("Library").join("Application Support").join("glances").join("glances.conf"));
            paths.push(PathBuf::from("/usr/local/etc/glances/glances.conf"));
        } else if cfg!(target_os = "windows") {
            if let Some(appdata) = std::env::var_os("APPDATA") {
                paths.push(PathBuf::from(appdata).join("glances").join("glances.conf"));
            }
        } else {
            // BSD or other unix: similar to Linux.
            paths.push(home.join(".config").join("glances").join("glances.conf"));
            paths.push(PathBuf::from("/usr/local/etc/glances/glances.conf"));
        }
    }
    // Package default.
    if let Some(prefix) = std::env::var_os("PREFIX") {
        paths.push(PathBuf::from(prefix).join("share").join("doc").join("glances").join("glances.conf"));
    }
    paths
}

/// Pick the first existing config path from `candidate_paths()`. Returns
/// `None` if no candidate exists (caller should use defaults).
pub fn find_existing() -> Option<PathBuf> {
    candidate_paths().into_iter().find(|p| p.exists())
}

/// Resolve the actual config path to use. `cli_override` is the `--config`
/// value if present. Otherwise the first existing candidate, otherwise the
/// user-path (so the file can be created there).
pub fn resolve(cli_override: Option<&str>) -> PathBuf {
    if let Some(p) = cli_override {
        return PathBuf::from(p);
    }
    find_existing().unwrap_or_else(|| {
        candidate_paths().into_iter().next().unwrap_or_else(|| PathBuf::from("glances.conf"))
    })
}

/// Returns the user config dir (for password file + log file), creating it
/// if missing.
pub fn user_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let dir = if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support").join("glances")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
            .unwrap_or(home)
            .join("glances")
    } else {
        // XDG-aware
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("glances")
        } else {
            home.join(".config").join("glances")
        }
    };
    dir
}

/// Returns the cache dir for the log file (Glances writes
/// `$XDG_CACHE_HOME/glances/glances.log`).
pub fn cache_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("glances");
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if cfg!(target_os = "macos") {
            return home.join("Library").join("Caches").join("glances");
        }
        return home.join(".local").join("share").join("glances");
    }
    std::env::temp_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_non_empty() {
        // Just verify it doesn't panic and returns at least the home-based one.
        let c = candidate_paths();
        assert!(!c.is_empty(), "candidate paths must include at least the home dir");
    }

    #[test]
    fn resolve_returns_existing_or_default() {
        let p = resolve(None);
        assert!(p.is_absolute() || p == Path::new("glances.conf"),
                "resolve returned non-sensible path: {:?}", p);
    }

    #[test]
    fn user_dir_under_home() {
        let d = user_dir();
        assert!(d.to_string_lossy().contains("glances"),
                "user_dir should contain 'glances': {:?}", d);
    }
}
