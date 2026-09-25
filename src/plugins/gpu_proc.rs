//! GPU client identity — `/proc` names, parents, media services.
//!
//! Shared by the NVIDIA (nvidia-smi pids) and open-driver (fdinfo
//! pids) attribution paths. A client's display name prefers its
//! `/proc` comm; its media service (jellyfin/emby/plex) matches the
//! own name first, then walks up to 3 ancestors so
//! `jellyfin -> ffmpeg` chains attribute to jellyfin.

use std::fs;
use std::path::Path;

/// Process short name from `/proc/<pid>/comm`.
pub fn proc_comm(proc_root: &Path, pid: u32) -> Option<String> {
    let s = fs::read_to_string(proc_root.join(pid.to_string()).join("comm")).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Parent pid from `/proc/<pid>/status`.
pub fn parent_pid(proc_root: &Path, pid: u32) -> Option<u32> {
    let text = fs::read_to_string(proc_root.join(pid.to_string()).join("status")).ok()?;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("PPid:") {
            return v.trim().parse::<u32>().ok();
        }
    }
    None
}

/// Identity text for service matching: comm plus the executable
/// path plus argv[0]. The full command line is deliberately NOT
/// included — arguments would false-positive (`grep plex` is not
/// Plex). Missing pieces are skipped, never errors.
pub fn proc_identity(proc_root: &Path, pid: u32) -> Option<String> {
    let dir = proc_root.join(pid.to_string());
    let mut parts = Vec::new();
    if let Some(comm) = proc_comm(proc_root, pid) {
        parts.push(comm);
    }
    if let Ok(exe) = fs::read_link(dir.join("exe")) {
        parts.push(exe.to_string_lossy().into_owned());
    }
    if let Ok(cmd) = fs::read(dir.join("cmdline")) {
        let arg0 = cmd.split(|b| *b == 0).next().unwrap_or(&[]);
        let arg0 = String::from_utf8_lossy(arg0).trim().to_string();
        if !arg0.is_empty() {
            parts.push(arg0);
        }
    }
    if parts.is_empty() { None } else { Some(parts.join(" ")) }
}

/// Service match over identity text (comm + exe path + argv[0]):
/// the media servers plus the AI stacks, whose processes often
/// hide behind generic names (`python` for InvokeAI).
pub fn service_from_name(text: &str) -> Option<&'static str> {
    let n = text.to_ascii_lowercase();
    if n.contains("jellyfin") {
        Some("jellyfin")
    } else if n.contains("emby") {
        Some("emby")
    } else if n.contains("plex") {
        Some("plex")
    } else if n.contains("invokeai") || n.contains("invoke_ai") {
        Some("invokeai")
    } else if n.contains("ollama") {
        Some("ollama")
    } else {
        None
    }
}

/// Transcoder-like process names: ffmpeg family (what Jellyfin,
/// Tdarr, Unmanic all spawn), Plex's transcriber, HandBrake, OBS
/// (all hardware-encode through the GPU when configured to).
pub fn is_transcoder_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["ffmpeg", "transcod", "handbrake", "jellyfin", "emby", "plex", "obs", "unmanic",
        "tdarr"]
        .iter()
        .any(|k| n.contains(k))
}

/// Resolve a GPU client's display name and service. The name
/// prefers `/proc` comm and falls back to the basename of `hint`
/// (nvidia-smi's full path). The service matches the own identity
/// (comm + exe + argv[0]) first, then walks up to 3 ancestors so
/// `jellyfin -> ffmpeg` chains attribute to jellyfin.
pub fn resolve_client(proc_root: &Path, pid: u32, hint: &str) -> (String, Option<String>) {
    let name = proc_comm(proc_root, pid).unwrap_or_else(|| {
        hint.rsplit('/').next().unwrap_or(hint).to_string()
    });
    if name.is_empty() {
        return ("?".to_string(), None);
    }
    let own = proc_identity(proc_root, pid).unwrap_or_else(|| name.clone());
    if let Some(s) = service_from_name(&own) {
        return (name, Some(s.to_string()));
    }
    let mut cur = pid;
    for _ in 0..3 {
        let Some(pp) = parent_pid(proc_root, cur) else { break };
        if pp == 0 || pp == cur {
            break;
        }
        cur = pp;
        if let Some(ident) = proc_identity(proc_root, cur)
            && let Some(s) = service_from_name(&ident) {
                return (name, Some(s.to_string()));
            }
    }
    (name, None)
}
