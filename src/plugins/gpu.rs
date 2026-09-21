//! GPU plugin — per-card vendor/util/freq from sysfs.
//!
//! Linux exposes /sys/class/drm/cardN with one entry per DRM device. The
//! `device/driver` symlink tells us the vendor (amdgpu, i915, nouveau,
//! tegra, etc.). Each vendor publishes its own utilization + frequency
//! files; the plugin probes the well-known paths and leaves fields Null
//! when the vendor doesn't expose them.
//!
//! Output is a `Value::Array` of `Value::Object`s keyed by `gpu_id`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::plugins::gpu_format::{assign_gpu_ids, gpu_to_value};
use crate::plugins::gpu_nvidia;

pub const NAME: &str = "gpu";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(GpuPlugin::new()));
}

const DRM_ROOT: &str = "/sys/class/drm";

/// Per-GPU record after sysfs probing. Pure data — the formatter
/// consumes it and emits a Value. Exposed for tests.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct GpuInfo {
    pub gpu_id: String,
    pub pci: String,
    pub vendor: String,
    pub name: String,
    pub kind: String,
    pub util_pct: Option<f64>,
    pub freq_mhz: Option<f64>,
    pub mem_used_mb: Option<f64>,
    pub mem_total_mb: Option<f64>,
    pub temp_c: Option<f64>,
}

fn read_link_basename(path: &Path) -> Option<String> {
    let target = fs::read_link(path).ok()?;
    let s = target.to_string_lossy().into_owned();
    let last = s.rsplit('/').next()?.to_string();
    if last.is_empty() { None } else { Some(last) }
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_f64(path: &Path) -> Option<f64> {
    read_trimmed(path)?.parse::<f64>().ok()
}

/// Vendor detection by reading the driver symlink under
/// /sys/class/drm/cardN/device/driver.
pub fn detect_vendor(card_dir: &Path) -> String {
    let driver_link = card_dir.join("device").join("driver");
    let raw = match fs::read_link(&driver_link) {
        Ok(t) => t.to_string_lossy().into_owned(),
        Err(_) => return "unknown".to_string(),
    };
    // `../../../../bus/pci/drivers/amdgpu` → "amdgpu"
    let base = raw.rsplit('/').next().unwrap_or("").to_string();
    vendor_from_driver(&base)
}

pub fn vendor_from_driver(driver: &str) -> String {
    match driver {
        "amdgpu" => "amd".to_string(),
        "i915" => "intel".to_string(),
        "nouveau" => "nvidia".to_string(),
        "tegra-drm" | "tegra" => "tegra".to_string(),
        "vmwgfx" => "vmware".to_string(),
        "vboxvideo" => "vbox".to_string(),
        "radeon" => "amd".to_string(),
        other => other.to_string(),
    }
}

/// Read utilisation percentage from the vendor-appropriate path.
/// AMD: `<dev>/gpu_busy_percent` (or `device/gpu_utilization` on older
///      drivers — accept either as integer percent).
/// Intel: no canonical utilization file; only frequencies available,
///        so we leave util_pct as None.
/// NVIDIA (nouveau): no standard utilization file; same fallback.
/// Tegra: no standard utilization file; same fallback.
pub fn read_util(card_dir: &Path, vendor: &str) -> Option<f64> {
    let dev = card_dir.join("device");
    match vendor {
        "amd" => {
            // Modern amdgpu uses `gpu_busy_percent` (0-100). Older
            // kernels only had `gpu_utilization` with the same scale.
            if let Some(v) = read_f64(&dev.join("gpu_busy_percent")) {
                return Some(v);
            }
            read_f64(&dev.join("gpu_utilization"))
        }
        // Intel/NVIDIA/Tegra fallback: leave None. We deliberately
        // don't try to compute utilization from frequency readings —
        // those are meaningless without a max and a load.
        _ => None,
    }
}

/// Read current frequency (MHz). Vendor-specific paths:
/// Intel: `<dev>/gt_cur_freq_mhz` (some kernels) or
///        `<dev>/gt/gt0/rps_cur_freq`.
/// AMD:   frequency is under `pp_dpm_sclk` (text); we instead use
///        `pp_sclk_od` or `power/cur_freq` if present — these are
///        optional and frequently absent.
pub fn read_freq_mhz(card_dir: &Path, vendor: &str) -> Option<f64> {
    let dev = card_dir.join("device");
    match vendor {
        "intel" => {
            if let Some(v) = read_f64(&dev.join("gt_cur_freq_mhz")) {
                return Some(v);
            }
            read_f64(&dev.join("gt").join("gt0").join("rps_cur_freq"))
        }
        "amd" => {
            // power/cur_freq gives frequency in Hz on some kernels.
            if let Some(hz) = read_f64(&dev.join("power").join("cur_freq")) {
                return Some(hz / 1_000_000.0);
            }
            // pp_sclk_od reports MHz but is typically 0; skip.
            None
        }
        _ => None,
    }
}

/// Internal (integrated) vs external (discrete) classification.
/// Firmware `label` ("Onboard - Video") is authoritative when present;
/// Intel fixes its iGPU at PCI 00:02.x across generations (Arc dGPUs
/// live elsewhere); Tegra is always SoC-integrated. Anything else is
/// external. Limitation: AMD APUs without a firmware label classify
/// as external.
pub fn classify_kind(vendor: &str, pci_id: &str, label: Option<&str>) -> &'static str {
    if let Some(l) = label {
        let low = l.to_ascii_lowercase();
        if low.contains("onboard") || low.contains("integrated") { return "internal"; }
    }
    if vendor == "tegra" { return "internal"; }
    if vendor == "intel" {
        let pci = pci_id.strip_prefix("0000:").unwrap_or(pci_id);
        if pci == "00:02.0" || pci.starts_with("00:02.") { return "internal"; }
    }
    "external"
}

/// Sort internal GPUs first, then by name — stable dashboard order.
pub fn sort_gpus(gpus: &mut [GpuInfo]) {
    gpus.sort_by(|a, b| {
        let ka = u8::from(a.kind != "internal");
        let kb = u8::from(b.kind != "internal");
        (ka, &a.name).cmp(&(kb, &b.name))
    });
}

/// List `/sys/class/drm/cardN` directories (skips connectors and the
/// `renderD*` nodes — those aren't GPU cards).
pub fn list_cards() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(DRM_ROOT) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.starts_with("card") && !name.contains('-') {
            out.push(ent.path());
        }
    }
    out.sort();
    out
}

/// Probe one card directory and return its GpuInfo. Exposed so tests
/// can build a fake sysfs tree and exercise the formatter.
pub fn probe_card(card_dir: &Path) -> GpuInfo {
    let fallback = card_dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "gpu0".to_string());
    let vendor = detect_vendor(card_dir);
    let label = read_trimmed(&card_dir.join("device").join("label")).filter(|s| !s.is_empty());
    let name = label.clone().unwrap_or_else(|| fallback.clone());
    let pci = read_link_basename(&card_dir.join("device"))
        .unwrap_or_else(|| fallback.clone());
    let kind = classify_kind(&vendor, &pci, label.as_deref()).to_string();
    let util_pct = read_util(card_dir, &vendor);
    let freq_mhz = read_freq_mhz(card_dir, &vendor);
    // Upstream-style `gpu_id` (`nvidia0`, ...) is assigned in
    // update() once all cards are enumerated (see gpu_format).
    GpuInfo {
        gpu_id: String::new(), pci, vendor, name, kind, util_pct, freq_mhz,
        mem_used_mb: None, mem_total_mb: None, temp_c: None,
    }
}

pub struct GpuPlugin { base: GlancesPluginModel }

impl GpuPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

impl Default for GpuPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for GpuPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["proc", "mem"] }
    fn get_key(&self) -> Option<&'static str> { Some("gpu_id") }

    fn update(&mut self) -> Result<()> {
        let cards = list_cards();
        let mut infos: Vec<GpuInfo> = cards.iter().map(|p| probe_card(p)).collect();
        assign_gpu_ids(&mut infos);
        // One nvidia-smi call per tick, only when an NVIDIA card exists.
        if infos.iter().any(|g| g.vendor == "nvidia") {
            gpu_nvidia::apply(&gpu_nvidia::query_nvidia_smi(), &mut infos);
        }
        sort_gpus(&mut infos);
        let out: Vec<Value> = infos.iter().map(gpu_to_value).collect();
        self.base.stats = Value::Array(out);
        Ok(())
    }
}
