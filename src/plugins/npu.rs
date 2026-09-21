//! NPU plugin — Neural Processing Unit stats from sysfs.
//!
//! NPUs (AMD Ryzen AI / XDNA, Intel Meteor Lake NPU, etc.) show up as
//! PCI devices or platform devices. We flat-scan /sys/bus/pci/devices
//! and /sys/bus/platform/devices (name prefix or bound driver) and
//! read utilization / frequency files where they exist. A deep walk of
//! /sys/devices costs ~15s per tick, so we never do that; the device
//! list is scanned once and cached (NPU topology is boot-stable).
//!
//! On hosts without an NPU the plugin emits an empty array — never an
//! error — so the JSON shape stays stable.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "npu";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(NpuPlugin::new()));
}

/// Bus roots flat-scanned for NPU devices. Both are one level deep
/// (~hundreds of entries), unlike /sys/devices (tens of thousands).
const BUS_ROOTS: &[&str] = &["/sys/bus/pci/devices", "/sys/bus/platform/devices"];

#[derive(Debug, Default, Clone, PartialEq)]
pub struct NpuInfo {
    pub npu_id: String,
    pub path: String,
    pub vendor: String,
    pub util_pct: Option<f64>,
    pub freq_mhz: Option<f64>,
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_f64(path: &Path) -> Option<f64> {
    read_trimmed(path)?.parse::<f64>().ok()
}

/// Driver basenames that indicate an NPU/accelerator device.
fn is_npu_driver(driver: &str) -> bool {
    matches!(driver, "amdxdna" | "accel" | "vaim" | "intel_vpu" | "ivpu")
}

/// Flat, non-recursive scan of one bus root: match by directory-name
/// prefix (`npu*`, `ai_accel*`) or by bound driver. Substring matches
/// (e.g. `/input` matches `npu`) would false-positive, so the name
/// check is prefix-only.
pub fn scan_bus_root(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.is_empty() { continue; }
        if name.to_ascii_lowercase().starts_with("npu")
            || name.to_ascii_lowercase().starts_with("ai_accel")
        {
            out.push(ent.path());
            continue;
        }
        if let Ok(target) = fs::read_link(ent.path().join("driver")) {
            let base = target.to_string_lossy().into_owned();
            if is_npu_driver(base.rsplit('/').next().unwrap_or("")) {
                out.push(ent.path());
            }
        }
    }
}

pub fn list_npu() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in BUS_ROOTS {
        scan_bus_root(Path::new(root), &mut out);
    }
    out.sort();
    out
}

/// Best-effort vendor identification from the parent's driver
/// symlink. Falls back to "unknown".
pub fn detect_vendor(dev_dir: &Path) -> String {
    // Look up the chain: /sys/devices/.../foo → ../<driver> via the
    // device's `driver` symlink.
    if let Ok(target) = fs::read_link(dev_dir.join("driver")) {
        let s = target.to_string_lossy().into_owned();
        if let Some(last) = s.rsplit('/').next() {
            return vendor_from_driver(last);
        }
    }
    // Fallback: walk up to the PCI parent and read its driver link.
    let mut p = dev_dir.to_path_buf();
    for _ in 0..6 {
        if !p.pop() { break; }
        if let Ok(target) = fs::read_link(p.join("driver")) {
            let s = target.to_string_lossy().into_owned();
            if let Some(last) = s.rsplit('/').next() {
                return vendor_from_driver(last);
            }
        }
    }
    "unknown".to_string()
}

pub fn vendor_from_driver(driver: &str) -> String {
    match driver {
        "amdxdna" => "amd".to_string(),
        "accel" | "vaim" | "intel_vpu" | "ivpu" => "intel".to_string(),
        _ => "unknown".to_string(),
    }
}

fn read_util(dev_dir: &Path) -> Option<f64> {
    // AMD XDNA exposes utilisation as either a percent or a load
    // value. Accept both, picking the first one that parses.
    for candidate in ["util_pct", "utilization", "busy_percent", "load"] {
        if let Some(v) = read_f64(&dev_dir.join(candidate)) {
            // Heuristic: anything > 100 is normalised load, not %.
            if v <= 100.0 { return Some(v); }
        }
    }
    None
}

fn read_freq_mhz(dev_dir: &Path) -> Option<f64> {
    // Look for any *_freq_mhz / *_cur_freq file.
    let entries = match fs::read_dir(dev_dir) {
        Ok(e) => e,
        Err(_) => return None,
    };
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        if !(lower.contains("freq")) { continue; }
        if let Some(v) = read_f64(&ent.path()) {
            // Heuristic: anything >= 1e6 is in Hz — convert to MHz.
            // (Kernel exposes Hz in `pp_dpm_*` files for GPUs but NPU
            // drivers typically already report MHz.)
            if v >= 1_000_000.0 { return Some(v / 1_000_000.0); }
            return Some(v);
        }
    }
    None
}

pub fn probe_npu(dev_dir: &Path) -> NpuInfo {
    let name = dev_dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "npu0".to_string());
    let vendor = detect_vendor(dev_dir);
    let util_pct = read_util(dev_dir);
    let freq_mhz = read_freq_mhz(dev_dir);
    NpuInfo {
        npu_id: name,
        path: dev_dir.to_string_lossy().into_owned(),
        vendor,
        util_pct,
        freq_mhz,
    }
}

pub fn npu_to_value(n: &NpuInfo) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("npu_id".into(), Value::String(n.npu_id.clone()));
    obj.insert("vendor".into(), Value::String(n.vendor.clone()));
    obj.insert("util_pct".into(), match n.util_pct {
        Some(v) => Value::Float(v),
        None => Value::Null,
    });
    obj.insert("freq_mhz".into(), match n.freq_mhz {
        Some(v) => Value::Float(v),
        None => Value::Null,
    });
    Value::Object(obj)
}

pub struct NpuPlugin {
    base: GlancesPluginModel,
    /// Device list scanned once — NPU topology is boot-stable and
    /// re-scanning sysfs every tick is pure overhead.
    devices: Option<Vec<PathBuf>>,
}

impl NpuPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())), devices: None }
    }
}

impl Default for NpuPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for NpuPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["freq", "load", "mem"] }
    fn get_key(&self) -> Option<&'static str> { Some("npu_id") }

    fn update(&mut self) -> Result<()> {
        if self.devices.is_none() {
            self.devices = Some(list_npu());
        }
        let empty = Vec::new();
        let npus = self.devices.as_ref().unwrap_or(&empty);
        let out: Vec<Value> = npus.iter().map(|p| npu_to_value(&probe_npu(p))).collect();
        self.base.stats = Value::Array(out);
        Ok(())
    }
}
