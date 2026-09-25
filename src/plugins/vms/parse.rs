//! virsh/multipass output parsers (fixture-testable, no binaries needed).

use std::collections::HashMap;
use std::process::Command;

/// One VM row (engine-specific fields are `None` when unavailable).
#[derive(Debug, Clone, Default)]
pub struct VmRow {
    pub name: String,
    pub status: String,
    pub engine: String,
    pub engine_version: String,
    pub cpu_count: Option<u64>,
    pub cpu_percent: Option<f64>,
    pub memory_usage: Option<u64>,
    pub memory_total: Option<u64>,
    pub ipv4: Option<String>,
}

/// Locate a helper binary without a shell.
/// `GLANCES_HELPER_DIR`, when set, is searched first — a hook for
/// tests (stub binaries) and debugging; default lookup is unchanged.
pub(crate) fn find_bin(candidates: &[&str], name: &str) -> Option<String> {
    if let Ok(dir) = std::env::var("GLANCES_HELPER_DIR") {
        let full = format!("{dir}/{name}");
        if std::path::Path::new(&full).is_file() {
            return Some(full);
        }
    }
    for dir in candidates {
        let full = format!("{}/{}", dir, name);
        if std::path::Path::new(&full).is_file() {
            return Some(full);
        }
    }
    None
}

/// Run a helper argv-only, returning stdout on success.
pub(crate) fn run(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Parse `virsh list --all` rows into (name, state).
/// Rows follow the `-----` separator: `<id> <name> <state...>` where the
/// id may be `-` for inactive domains.
pub fn parse_virsh_list(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut started = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !started {
            if !trimmed.is_empty() && trimmed.chars().all(|c| c == '-' || c == ' ') {
                started = true;
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let _id = match parts.next() {
            Some(i) => i,
            None => continue,
        };
        let name = match parts.next() {
            Some(n) => n,
            None => continue,
        };
        let state: Vec<&str> = parts.collect();
        out.push((name.to_string(), state.join(" ")));
    }
    out
}

/// Parse `virsh domstats` into name → (key → value) maps.
pub fn parse_domstats(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Domain:") {
            let name = rest.trim().trim_matches('\'').to_string();
            current = Some(name.clone());
            out.entry(name).or_default();
        } else if let Some((k, v)) = trimmed.split_once('=')
            && let Some(name) = &current {
                out.entry(name.clone())
                    .or_default()
                    .insert(k.trim().to_string(), v.trim().to_string());
            }
    }
    out
}

/// Parse `multipass list --format csv` rows into
/// (name, state, ipv4, release). First line is the header.
pub fn parse_multipass_csv(text: &str) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 4 {
            continue;
        }
        out.push((
            parts[0].trim().to_string(),
            parts[1].trim().to_string(),
            parts[2].trim().to_string(),
            parts[3..].join(",").trim().to_string(),
        ));
    }
    out
}

