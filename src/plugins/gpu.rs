//! GPU plugin — per-card vendor/util/freq from sysfs.
//!
//! Linux exposes /sys/class/drm/cardN with one entry per DRM device. The
//! `device/driver` symlink tells us the vendor (amdgpu, i915, nouveau,
//! tegra, etc.). Each vendor publishes its own utilization + frequency
//! files; the plugin probes the well-known paths and leaves fields Null
//! when the vendor doesn't expose them.
//!
//! Output is a `Value::Array` of `Value::Object`s keyed by `gpu_id`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::plugins::gpu_drm;
use crate::plugins::gpu_format::{assign_gpu_ids, gpu_to_value, sort_gpus};
use crate::plugins::gpu_nvidia;
use crate::plugins::gpu_sysfs;

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
    /// Sysfs card node (`card1`) — internal join key for DRM
    /// clients, not emitted (gpu_id + pci already identify).
    pub card: String,
    pub clients: Vec<gpu_drm::GpuClient>,
    pub transcoding: bool,
    pub transcoding_by: Option<String>,
}

fn read_link_basename(path: &Path) -> Option<String> {
    let target = fs::read_link(path).ok()?;
    let s = target.to_string_lossy().into_owned();
    let last = s.rsplit('/').next()?.to_string();
    if last.is_empty() { None } else { Some(last) }
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
    let label = gpu_sysfs::read_trimmed(&card_dir.join("device").join("label"))
        .filter(|s| !s.is_empty());
    let name = label.clone().unwrap_or_else(|| fallback.clone());
    let pci = read_link_basename(&card_dir.join("device"))
        .unwrap_or_else(|| fallback.clone());
    let kind = classify_kind(&vendor, &pci, label.as_deref()).to_string();
    let util_pct = gpu_sysfs::read_util(card_dir, &vendor);
    let freq_mhz = gpu_sysfs::read_freq_mhz(card_dir, &vendor);
    // Upstream-style `gpu_id` (`nvidia0`, ...) is assigned in
    // update() once all cards are enumerated (see gpu_format).
    GpuInfo {
        gpu_id: String::new(), pci, vendor, name, kind, util_pct, freq_mhz,
        mem_used_mb: None, mem_total_mb: None, temp_c: None,
        card: fallback, clients: Vec::new(), transcoding: false, transcoding_by: None,
    }
}

pub struct GpuPlugin {
    base: GlancesPluginModel,
    /// Previous-tick fdinfo engine counters in ns, keyed by
    /// (pid, engine). Activity is tick-over-tick advance, same
    /// idea as the network plugin's byte rates.
    prev_engines: HashMap<(u32, String), u64>,
    /// Last `nvidia-smi` sweep, re-applied on ticks inside the
    /// freshness window so the binary runs ~1/3 as often.
    last_rows: Vec<gpu_nvidia::NvidiaSmiRow>,
    last_apps: Vec<gpu_nvidia::NvidiaApp>,
    last_nvidia_at: Option<std::time::Instant>,
}

impl GpuPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            prev_engines: HashMap::new(),
            last_rows: Vec::new(),
            last_apps: Vec::new(),
            last_nvidia_at: None,
        }
    }
}

impl Default for GpuPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for GpuPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) {
        self.base.reset();
        self.prev_engines.clear();
        self.last_rows.clear();
        self.last_apps.clear();
        self.last_nvidia_at = None;
    }
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
        // Two nvidia-smi calls per sweep (stats + compute clients),
        // only when an NVIDIA card exists; sweeps are cached for 6s.
        if infos.iter().any(|g| g.vendor == "nvidia") {
            let now = std::time::Instant::now();
            if !gpu_nvidia::nvidia_fresh(self.last_nvidia_at, now) {
                self.last_rows = gpu_nvidia::query_nvidia_smi();
                self.last_apps = gpu_nvidia::query_apps();
                self.last_nvidia_at = Some(now);
            }
            gpu_nvidia::apply(&self.last_rows, &mut infos);
            gpu_nvidia::apply_apps(
                &self.last_apps,
                &self.last_rows,
                &mut infos,
                Path::new("/proc"),
            );
        }
        // Open drivers (Intel/AMD/...) attribute via fdinfo
        // engine counters — skipped on NVIDIA-only machines.
        if infos.iter().any(|g| g.vendor != "nvidia") {
            let found = gpu_drm::scan_drm_clients(Path::new("/proc"), Path::new(DRM_ROOT));
            gpu_drm::attach_drm_clients(&mut self.prev_engines, &found, &mut infos);
        }
        sort_gpus(&mut infos);
        let out: Vec<Value> = infos.iter().map(gpu_to_value).collect();
        self.base.stats = Value::Array(out);
        Ok(())
    }
}
