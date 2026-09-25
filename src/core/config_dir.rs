//! Linux path resolution for config, user data, and cache.
//!
//! Order for the config file: explicit CLI path, then the user config
//! (`XDG_CONFIG_HOME` or `~/.config`), then the system file, then the
//! package fallback. Callers probe candidates in order.

use std::env;
use std::path::PathBuf;

fn home() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn config_home() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg));
    }
    home().map(|h| h.join(".config"))
}

/// Candidate config files in probe order. Empty without HOME (except a
/// PREFIX package fallback, which stands alone).
pub fn candidate_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(base) = config_home() {
        out.push(base.join("glances").join("glances.conf"));
    }
    if home().is_some() {
        out.push(PathBuf::from("/etc/glances/glances.conf"));
    }
    if let Some(prefix) = env::var_os("PREFIX") {
        out.push(PathBuf::from(prefix).join("share").join("doc").join("glances").join("glances.conf"));
    }
    out
}

/// First candidate that exists on disk, if any.
pub fn find_existing() -> Option<PathBuf> {
    candidate_paths().into_iter().find(|p| p.exists())
}

/// The config path to use: CLI override first, then the first existing
/// candidate, then the first candidate (so it can be created), else a
/// bare filename fallback.
pub fn resolve(cli_override: Option<&str>) -> PathBuf {
    if let Some(p) = cli_override {
        return PathBuf::from(p);
    }
    find_existing().unwrap_or_else(|| {
        candidate_paths().into_iter().next().unwrap_or_else(|| PathBuf::from("glances.conf"))
    })
}

/// User data dir (password + state files). Pure computation — the
/// caller creates it. Falls back to the current dir without HOME.
pub fn user_dir() -> PathBuf {
    config_home()
        .map(|c| c.join("glances"))
        .unwrap_or_else(|| home().map(|h| h.join(".config").join("glances")).unwrap_or_else(|| PathBuf::from(".")))
}

/// Cache dir for the log file: XDG cache home, else `~/.local/share`,
/// else the system temp dir.
pub fn cache_dir() -> PathBuf {
    if let Some(xdg) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("glances");
    }
    if let Some(h) = home() {
        return h.join(".local").join("share").join("glances");
    }
    env::temp_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn candidates_end_with_glances_conf() {
        for p in candidate_paths() {
            assert_eq!(p.file_name().and_then(|s| s.to_str()), Some("glances.conf"));
        }
    }
    #[test]
    fn override_wins_and_resolve_is_sensible() {
        assert_eq!(resolve(Some("/tmp/x.conf")), PathBuf::from("/tmp/x.conf"));
        let p = resolve(None);
        assert!(p.is_absolute() || p.as_path() == Path::new("glances.conf"));
    }
    #[test]
    fn dirs_carry_the_product_name() {
        assert!(user_dir().join("x").to_string_lossy().contains("glances"));
        assert!(cache_dir().join("x").to_string_lossy().contains("glances"));
    }
}
