//! GPU client attribution for open drivers (Intel/AMD/etc.).
//!
//! The kernel reports per-DRM-context engine time in
//! `/proc/<pid>/fdinfo/<fd>` (`drm-engine-video: 123 ns`, ...).
//! Scanning those counters answers "who is using the GPU" and —
//! when a *video* engine (decode/encode) advances for a transcoder
//! process — "is it transcoding". No userspace tools needed, just
//! file reads, so it works identically on host and container
//! (both share the host PID namespace).
//!
//! A client only counts as *active* when one of its engines
//! advances by >= 1ms of engine time between ticks; idle contexts
//! (a quiet desktop compositor holding a render node) stay silent.
//! Processes we cannot read (other users without privileges) are
//! skipped, never errors.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Minimum engine-time advance per tick to count a client as
/// active (1ms in ns). A sustained transcode advances tens of ms
/// per tick; a single still frame stays below this.
pub const ENGINE_ACTIVE_NS: u64 = 1_000_000;

/// One process holding DRM file(s) on a card, engines merged
/// across its fds (counters summed per engine name).
#[derive(Debug, Clone, PartialEq)]
pub struct DrmClient {
    pub pid: u32,
    pub card: String,
    pub engines: Vec<(String, u64)>,
}

/// Parse `drm-engine-<name>: <ns> ns` lines from fdinfo text.
/// Malformed lines are skipped; never fails.
pub fn parse_fdinfo_engines(text: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let Some(name) = k.trim().strip_prefix("drm-engine-") else { continue };
        let name = name.trim();
        if name.is_empty() { continue; }
        let Some(ns) = v.split_whitespace().next().and_then(|n| n.parse::<u64>().ok()) else {
            continue;
        };
        out.push((name.to_string(), ns));
    }
    out
}

/// Video-class engines (decode/encode) across vendors: i915
/// `video`/`video-enhance`, amdgpu `dec`/`enc`/`jpeg`, plus the
/// classic block names. Everything else (render/gfx/compute) is
/// general GPU use, not video.
pub fn is_video_engine(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("video")
        || n.contains("dec")
        || n.contains("enc")
        || n.contains("jpeg")
        || n.contains("vpe")
        || n.contains("vcn")
        || n.contains("uvd")
        || n.contains("vce")
}

/// Map a `/dev/dri` node name to its `cardN`. `cardN` maps to
/// itself; `renderDN` resolves by matching the backing PCI device
/// symlink against each card's (both links are relative to the
/// same directory, so plain string comparison is sound).
pub fn dri_node_to_card(sys_drm: &Path, node: &str) -> Option<String> {
    if node.contains('/') || node.contains('-') {
        return None;
    }
    if let Some(rest) = node.strip_prefix("card") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(node.to_string());
        }
        return None;
    }
    if !node.starts_with("renderD") {
        return None;
    }
    let want = fs::read_link(sys_drm.join(node).join("device")).ok()?;
    let entries = fs::read_dir(sys_drm).ok()?;
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        if let Ok(link) = fs::read_link(ent.path().join("device")) {
            if link == want {
                return Some(name);
            }
        }
    }
    None
}

/// Scan `proc_root` for processes holding `/dev/dri/*` fds and
/// read their fdinfo engine counters. Unreadable processes/fds
/// (permissions, races with exit) are skipped silently.
pub fn scan_drm_clients(proc_root: &Path, sys_drm: &Path) -> Vec<DrmClient> {
    let mut merged: BTreeMap<(u32, String), BTreeMap<String, u64>> = BTreeMap::new();
    let procs = match fs::read_dir(proc_root) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    for ent in procs.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let Ok(pid) = name.parse::<u32>() else { continue };
        let fd_dir = ent.path().join("fd");
        let fds = match fs::read_dir(&fd_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for fd in fds.flatten() {
            let target = match fs::read_link(fd.path()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let node = match target.to_str().and_then(|s| s.strip_prefix("/dev/dri/")) {
                Some(n) => n,
                None => continue,
            };
            let Some(card) = dri_node_to_card(sys_drm, node) else { continue };
            let info = ent.path().join("fdinfo").join(fd.file_name());
            let text = match fs::read_to_string(&info) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let slot = merged.entry((pid, card)).or_default();
            for (engine, ns) in parse_fdinfo_engines(&text) {
                *slot.entry(engine).or_default() += ns;
            }
        }
    }
    merged
        .into_iter()
        .map(|((pid, card), engines)| DrmClient {
            pid,
            card,
            engines: engines.into_iter().collect(),
        })
        .collect()
}

/// One process actively using a GPU: pid, short name, the
/// service it belongs to when known (jellyfin/emby/plex/invokeai/
/// ollama), its VRAM hold in MiB (NVIDIA only — fdinfo has no
/// memory counters, so open drivers report None), and whether it
/// is the one transcoding (so the dashboard never mixes a
/// transcoder and a bystander under one label).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct GpuClient {
    pub pid: u32,
    pub name: String,
    pub service: Option<String>,
    pub mem_mb: Option<f64>,
    pub transcoding: bool,
}

/// Attach open-driver clients to their cards. A client is listed
/// when any engine advanced >= 1ms since the previous tick; it
/// marks the card transcoding when a *video* engine advanced for
/// a transcoder-named process. `prev` is the previous tick's
/// (pid, engine) counters in ns — refreshed in place from this
/// tick's scan so dead pids drop out. First tick lists nothing
/// (no previous counters — same as network rates on tick one).
pub fn attach_drm_clients(
    prev: &mut std::collections::HashMap<(u32, String), u64>,
    found: &[DrmClient],
    infos: &mut [super::gpu::GpuInfo],
) {
    let mut by_client: BTreeMap<(&str, u32), Vec<(&str, u64)>> = BTreeMap::new();
    for c in found {
        for (e, ns) in &c.engines {
            by_client.entry((c.card.as_str(), c.pid)).or_default().push((e.as_str(), *ns));
        }
    }
    for ((card, pid), engines) in &by_client {
        let mut active_any = false;
        let mut active_video = false;
        for (e, ns) in engines {
            let delta = match prev.get(&(*pid, e.to_string())) {
                Some(p) if *ns >= *p => *ns - *p,
                // Unknown, wrapped, or pid-reused counter:
                // baseline it, report no activity this tick.
                _ => 0,
            };
            if delta >= ENGINE_ACTIVE_NS {
                active_any = true;
                if is_video_engine(e) {
                    active_video = true;
                }
            }
        }
        if !active_any {
            continue;
        }
        let proc_root = Path::new("/proc");
        let (name, service) = super::gpu_proc::resolve_client(proc_root, *pid, "");
        let Some(g) = infos.iter_mut().find(|g| g.card == *card) else { continue };
        let transcoding = active_video && super::gpu_proc::is_transcoder_name(&name);
        g.clients.push(GpuClient {
            pid: *pid,
            name: name.clone(),
            service: service.clone(),
            mem_mb: None,
            transcoding,
        });
        if transcoding {
            g.transcoding = true;
            if g.transcoding_by.is_none() {
                g.transcoding_by = Some(service.unwrap_or(name));
            }
        }
    }
    prev.clear();
    for c in found {
        for (e, ns) in &c.engines {
            prev.insert((c.pid, e.clone()), *ns);
        }
    }
}
