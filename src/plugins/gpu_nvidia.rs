//! NVIDIA live stats via `nvidia-smi` (argv-only, no shell).
//!
//! The proprietary NVIDIA driver exposes no utilization sysfs files,
//! so sysfs probing leaves util/freq Null. When an NVIDIA card is
//! present we run one `nvidia-smi --query-gpu=...` per tick (~40ms)
//! and join rows onto sysfs cards by PCI bus id. Any failure (binary
//! missing, driver wedged, unparsable output) yields no rows and the
//! sysfs Nulls stand — never an error, never a stall beyond the
//! subprocess itself.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::{Duration, Instant};

use super::gpu::GpuInfo;

/// Freshness window for an `nvidia-smi` stats+apps sweep: GPU
/// numbers move fast enough to watch but slow enough to sample,
/// so one sweep per 6s instead of per 2s tick.
const NVIDIA_TTL: Duration = Duration::from_secs(6);

/// True when a sweep taken at `at` is still inside the TTL window
/// at `now`. Split out so the boundary is unit-testable.
pub fn nvidia_fresh(at: Option<Instant>, now: Instant) -> bool {
    at.is_some_and(|t| now.duration_since(t) < NVIDIA_TTL)
}

/// One parsed `--query-gpu` row. Memory in MiB, temp in °C, clocks
/// in MHz, plus the product name (`nvidia-smi` prints no name for
/// the card otherwise — sysfs only gives `cardN`). Every counter is
/// Optional: `nvidia-smi` prints `[N/A]` for unsupported counters
/// and those must not clobber sysfs data.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct NvidiaSmiRow {
    pub pci: String,
    pub util_pct: Option<f64>,
    pub mem_used_mb: Option<f64>,
    pub mem_total_mb: Option<f64>,
    pub temp_c: Option<f64>,
    pub freq_mhz: Option<f64>,
    pub name: Option<String>,
    pub gpu_uuid: Option<String>,
}

/// Normalize PCI ids: `nvidia-smi` prints 8-digit domains
/// ("00000000:01:00.0"), sysfs uses 4 ("0000:01:00.0").
pub fn normalize_pci(id: &str) -> String {
    let id = id.trim();
    match id.split_once(':') {
        Some((dom, rest)) => {
            let dom = dom.trim_start_matches("0x").trim_start_matches('0');
            let d = u32::from_str_radix(if dom.is_empty() { "0" } else { dom }, 16).unwrap_or(0);
            format!("{:04x}:{}", d, rest)
        }
        None => id.to_string(),
    }
}

/// Parse `--format=csv,noheader,nounits` output. Accepts 7 fields
/// (no uuid, older callers) or 8 (with trailing `gpu_uuid`).
/// Malformed lines are skipped; unparsable fields become None;
/// never fails.
pub fn parse_nvidia_smi_csv(text: &str) -> Vec<NvidiaSmiRow> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        if f.len() != 7 && f.len() != 8 { continue; }
        let num = |s: &str| s.parse::<f64>().ok();
        out.push(NvidiaSmiRow {
            pci: normalize_pci(f[0]),
            util_pct: num(f[1]),
            mem_used_mb: num(f[2]),
            mem_total_mb: num(f[3]),
            temp_c: num(f[4]),
            freq_mhz: num(f[5]),
            name: match f[6] {
                "[N/A]" | "" => None,
                n => Some(n.to_string()),
            },
            gpu_uuid: f.get(7).and_then(|s| match *s {
                "[N/A]" | "" => None,
                u => Some(u.to_string()),
            }),
        });
    }
    out
}

/// Run one query. Empty vec on any failure (no binary, nonzero
/// status, unreadable output).
pub fn query_nvidia_smi() -> Vec<NvidiaSmiRow> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=pci.bus_id,utilization.gpu,memory.used,memory.total,temperature.gpu,clocks.current.graphics,name,gpu_uuid",
            "--format=csv,noheader,nounits",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => parse_nvidia_smi_csv(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// One `--query-compute-apps` row: a process holding a CUDA
/// context. `name` is the full binary path; `gpu_uuid` maps it to
/// a card (empty on drivers too old for the uuid field).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct NvidiaApp {
    pub pid: u32,
    pub name: String,
    pub mem_mb: Option<f64>,
    pub gpu_uuid: String,
}

/// Parse apps CSV (3 fields without uuid, 4 with). Paths
/// containing commas break the naive split, so over-long lines
/// are skipped rather than misattributed; never fails.
pub fn parse_apps_csv(text: &str) -> Vec<NvidiaApp> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        if f.len() != 3 && f.len() != 4 { continue; }
        let Ok(pid) = f[0].parse::<u32>() else { continue };
        if f[1].is_empty() { continue; }
        out.push(NvidiaApp {
            pid,
            name: f[1].to_string(),
            mem_mb: f[2].parse::<f64>().ok(),
            gpu_uuid: f.get(3).unwrap_or(&"").to_string(),
        });
    }
    out
}

/// Run the apps query. Empty vec on any failure.
pub fn query_apps() -> Vec<NvidiaApp> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-compute-apps=pid,process_name,used_memory,gpu_uuid",
            "--format=csv,noheader,nounits",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => parse_apps_csv(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// Attach compute clients to their cards via uuid, resolving each
/// pid to a display name + media service. Apps without a uuid
/// (old drivers) land on the single NVIDIA card when there is
/// exactly one, and are dropped when the target is ambiguous.
/// A transcoder-named client marks its card transcoding.
pub fn apply_apps(
    apps: &[NvidiaApp],
    rows: &[NvidiaSmiRow],
    infos: &mut [GpuInfo],
    proc_root: &std::path::Path,
) {
    use std::collections::BTreeMap;
    let uuid_pci: BTreeMap<&str, &str> = rows
        .iter()
        .filter_map(|r| r.gpu_uuid.as_deref().map(|u| (u, r.pci.as_str())))
        .collect();
    let nvidia_idx: Vec<usize> = infos
        .iter()
        .enumerate()
        .filter(|(_, g)| g.vendor == "nvidia")
        .map(|(i, _)| i)
        .collect();
    for app in apps {
        let idx = match uuid_pci.get(app.gpu_uuid.as_str()) {
            Some(p) => infos.iter().position(|g| g.vendor == "nvidia" && g.pci == **p),
            None if app.gpu_uuid.is_empty() && nvidia_idx.len() == 1 => Some(nvidia_idx[0]),
            None => None,
        };
        let Some(i) = idx else { continue };
        let g = &mut infos[i];
        let (name, service) = super::gpu_proc::resolve_client(proc_root, app.pid, &app.name);
        let transcoding = super::gpu_proc::is_transcoder_name(&name);
        g.clients.push(super::gpu_drm::GpuClient {
            pid: app.pid,
            name: name.clone(),
            service: service.clone(),
            mem_mb: app.mem_mb,
            transcoding,
        });
        if transcoding {
            g.transcoding = true;
            if g.transcoding_by.is_none() {
                g.transcoding_by = Some(service.unwrap_or(name));
            }
        }
    }
}

/// Join live rows onto sysfs-probed cards by normalized PCI id.
/// Only `Some` fields overwrite, so `[N/A]` counters never clobber.
pub fn apply(rows: &[NvidiaSmiRow], infos: &mut [GpuInfo]) {
    let map: BTreeMap<&str, &NvidiaSmiRow> = rows.iter().map(|r| (r.pci.as_str(), r)).collect();
    for g in infos.iter_mut() {
        let norm = normalize_pci(&g.pci);
        let Some(r) = map.get(norm.as_str()) else { continue };
        if r.util_pct.is_some() { g.util_pct = r.util_pct; }
        if r.freq_mhz.is_some() { g.freq_mhz = r.freq_mhz; }
        if r.mem_used_mb.is_some() { g.mem_used_mb = r.mem_used_mb; }
        if r.mem_total_mb.is_some() { g.mem_total_mb = r.mem_total_mb; }
        if r.temp_c.is_some() { g.temp_c = r.temp_c; }
        if let Some(n) = r.name.as_deref() { g.name = n.to_string(); }
    }
}
