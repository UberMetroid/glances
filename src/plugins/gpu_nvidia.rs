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

use super::gpu::GpuInfo;

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

/// Parse `--format=csv,noheader,nounits` output. Malformed lines are
/// skipped; unparsable fields become None; never fails.
pub fn parse_nvidia_smi_csv(text: &str) -> Vec<NvidiaSmiRow> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        if f.len() != 7 { continue; }
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
        });
    }
    out
}

/// Run one query. Empty vec on any failure (no binary, nonzero
/// status, unreadable output).
pub fn query_nvidia_smi() -> Vec<NvidiaSmiRow> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=pci.bus_id,utilization.gpu,memory.used,memory.total,temperature.gpu,clocks.current.graphics,name",
            "--format=csv,noheader,nounits",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => parse_nvidia_smi_csv(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
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
