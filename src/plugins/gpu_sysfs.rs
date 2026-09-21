//! GPU sysfs readers — utilization + frequency files.
//!
//! Each vendor publishes its own utilization/frequency files under
//! `/sys/class/drm/cardN/device`; these probes read the well-known
//! paths and return None when the vendor doesn't expose them.

use std::fs;
use std::path::Path;

pub fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_f64(path: &Path) -> Option<f64> {
    read_trimmed(path)?.parse::<f64>().ok()
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
