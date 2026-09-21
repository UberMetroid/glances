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

/// Media-server match by process name (own or ancestor's).
pub fn service_from_name(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    if n.contains("jellyfin") {
        Some("jellyfin")
    } else if n.contains("emby") {
        Some("emby")
    } else if n.contains("plex") {
        Some("plex")
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

/// Resolve a GPU client's display name and media service. The name
/// prefers `/proc` comm and falls back to the basename of `hint`
/// (nvidia-smi's full path). The service matches the own name
/// first, then walks up to 3 ancestors.
pub fn resolve_client(proc_root: &Path, pid: u32, hint: &str) -> (String, Option<String>) {
    let name = proc_comm(proc_root, pid).unwrap_or_else(|| {
        hint.rsplit('/').next().unwrap_or(hint).to_string()
    });
    if name.is_empty() {
        return ("?".to_string(), None);
    }
    if let Some(s) = service_from_name(&name) {
        return (name, Some(s.to_string()));
    }
    let mut cur = pid;
    for _ in 0..3 {
        let Some(pp) = parent_pid(proc_root, cur) else { break };
        if pp == 0 || pp == cur {
            break;
        }
        cur = pp;
        if let Some(pname) = proc_comm(proc_root, cur) {
            if let Some(s) = service_from_name(&pname) {
                return (name, Some(s.to_string()));
            }
        }
    }
    (name, None)
}
