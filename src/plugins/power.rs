//! Power plugin — watts per part plus an optional monthly cost.
//! Sources (best-effort; missing reads stay Null): CPU package via
//! the RAPL energy counter (needs root, first tick Null), NVIDIA via
//! a cached `nvidia-smi power.draw` sweep (30s TTL), AMD via amdgpu
//! hwmon. `total_watts` is measured parts, not wall power. The rate
//! comes from `GLANCES_KWH_RATE` or `[power] kwh_rate` (env wins).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "power";

/// Wall-clock freshness for the `nvidia-smi` power sweep.
const NVIDIA_TTL: Duration = Duration::from_secs(30);

/// Env var carrying the electricity price in dollars per kWh.
pub const KWH_RATE_ENV: &str = "GLANCES_KWH_RATE";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(PowerPlugin::new()));
}

/// Watts from a RAPL delta (µJ/s); None on reset or zero time.
pub fn watts_from_delta(prev_uj: u64, cur_uj: u64, dt_secs: f64) -> Option<f64> {
    if dt_secs <= 0.0 || cur_uj < prev_uj {
        return None;
    }
    Some((cur_uj - prev_uj) as f64 / dt_secs / 1_000_000.0)
}

/// Sum `nvidia-smi power.draw` CSV watts; None when none parsed.
pub fn parse_power_draw_csv(text: &str) -> Option<f64> {
    let mut sum = 0.0;
    let mut n = 0u32;
    for line in text.lines() {
        if let Ok(w) = line.trim().parse::<f64>() {
            if w.is_finite() && w >= 0.0 {
                sum += w;
                n += 1;
            }
        }
    }
    if n > 0 { Some(sum) } else { None }
}

/// Validated dollars-per-kWh from raw text; None when unusable.
pub fn parse_kwh_rate(raw: Option<&str>) -> Option<f64> {
    let r: f64 = raw?.trim().parse().ok()?;
    if r.is_finite() && r > 0.0 { Some(r) } else { None }
}

/// Monthly cost for a constant wattage: W * 24h * 30d / 1000 * rate.
pub fn usd_per_month(watts: f64, rate: f64) -> f64 {
    watts * 24.0 * 30.0 / 1000.0 * rate
}

fn read_rapl_uj() -> Option<u64> {
    fs::read_to_string("/sys/class/powercap/intel-rapl:0/energy_uj").ok()?.trim().parse().ok()
}

/// Sum amdgpu `power1_average` (µW) under `drm_root` (`/sys/class/drm` live).
pub fn amdgpu_watts(drm_root: &Path) -> Option<f64> {
    let mut sum_uw = 0u64;
    let mut n = 0u32;
    let entries = fs::read_dir(drm_root).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') { continue; }
        let is_amd = fs::read_link(entry.path().join("device/driver"))
            .ok()
            .and_then(|t| t.file_name().map(|s| s.to_owned()))
            .is_some_and(|s| s == "amdgpu");
        if !is_amd { continue; }
        let hwmon = entry.path().join("device/hwmon");
        for h in fs::read_dir(&hwmon).into_iter().flatten().flatten() {
            let text = fs::read_to_string(h.path().join("power1_average")).unwrap_or_default();
            if let Ok(uw) = text.trim().parse::<u64>() {
                sum_uw += uw;
                n += 1;
            }
        }
    }
    if n > 0 { Some(sum_uw as f64 / 1_000_000.0) } else { None }
}

fn query_nvidia_watts() -> Option<f64> {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=power.draw", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_power_draw_csv(&String::from_utf8_lossy(&out.stdout))
}

fn num_or_null(v: Option<f64>) -> Value {
    v.map(Value::Float).unwrap_or(Value::Null)
}

pub struct PowerPlugin {
    base: GlancesPluginModel,
    prev_uj: Option<u64>,
    prev_at: Option<Instant>,
    nv_at: Option<Instant>,
    nv_watts: Option<f64>,
    cfg_rate: Option<f64>,
}

impl PowerPlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        for k in ["cpu_watts", "gpu_watts", "total_watts", "kwh_rate", "usd_per_month"] {
            m.insert(k.into(), Value::Null);
        }
        m.insert("sources".into(), Value::Array(Vec::new()));
        Self {
            base: GlancesPluginModel::new(NAME, Value::Object(m)),
            prev_uj: None,
            prev_at: None,
            nv_at: None,
            nv_watts: None,
            cfg_rate: None,
        }
    }
}

/// Rate order: valid env wins, valid config falls back.
pub fn resolve_rate(env: Option<&str>, cfg: Option<f64>) -> Option<f64> {
    parse_kwh_rate(env).or(cfg.filter(|r| r.is_finite() && *r > 0.0))
}

impl Default for PowerPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for PowerPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn set_kwh_rate(&mut self, rate: Option<f64>) { self.cfg_rate = rate; }

    fn update(&mut self) -> Result<()> {
        let now = Instant::now();
        let cpu = read_rapl_uj().and_then(|cur| {
            let w = match (self.prev_uj, self.prev_at) {
                (Some(p), Some(t)) => watts_from_delta(p, cur, now.duration_since(t).as_secs_f64()),
                _ => None,
            };
            self.prev_uj = Some(cur);
            self.prev_at = Some(now);
            w
        });
        if self.nv_at.map_or(true, |t| now.duration_since(t) >= NVIDIA_TTL) {
            self.nv_watts = query_nvidia_watts();
            self.nv_at = Some(now);
        }
        let amd = amdgpu_watts(Path::new("/sys/class/drm"));
        let gpu = match (self.nv_watts, amd) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        };
        let total = match (cpu, gpu) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        };
        let rate = resolve_rate(std::env::var(KWH_RATE_ENV).ok().as_deref(), self.cfg_rate);
        let usd = match (total, rate) {
            (Some(w), Some(r)) => Some(usd_per_month(w, r)),
            _ => None,
        };
        let mut sources = Vec::new();
        if cpu.is_some() { sources.push(Value::String("rapl".into())); }
        if self.nv_watts.is_some() { sources.push(Value::String("nvidia".into())); }
        if amd.is_some() { sources.push(Value::String("amdgpu".into())); }
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("cpu_watts".into(), num_or_null(cpu));
            obj.insert("gpu_watts".into(), num_or_null(gpu));
            obj.insert("total_watts".into(), num_or_null(total));
            obj.insert("kwh_rate".into(), num_or_null(rate));
            obj.insert("usd_per_month".into(), num_or_null(usd));
            obj.insert("sources".into(), Value::Array(sources));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_math_and_guards() {
        assert_eq!(watts_from_delta(0, 2_000_000, 2.0), Some(1.0));
        assert_eq!(watts_from_delta(5, 3, 2.0), None);
        assert_eq!(watts_from_delta(5, 5, 0.0), None);
    }

    #[test]
    fn power_csv_sums_and_skips_na() {
        assert_eq!(parse_power_draw_csv("12.66\n14.49\n"), Some(27.15));
        assert_eq!(parse_power_draw_csv("[N/A]\n10.0\n"), Some(10.0));
        assert_eq!(parse_power_draw_csv("[N/A]\n"), None);
    }

    #[test]
    fn rate_and_monthly_cost() {
        assert_eq!(parse_kwh_rate(Some("0.30")), Some(0.3));
        assert_eq!(parse_kwh_rate(None), None);
        assert_eq!(parse_kwh_rate(Some("junk")), None);
        assert_eq!(parse_kwh_rate(Some("-1")), None);
        assert!((usd_per_month(100.0, 0.30) - 21.6).abs() < 1e-9);
    }

    #[test]
    fn rate_resolution_order() {
        assert_eq!(resolve_rate(Some("0.50"), Some(0.08)), Some(0.50));
        assert_eq!(resolve_rate(None, Some(0.08)), Some(0.08));
        assert_eq!(resolve_rate(Some("junk"), Some(0.08)), Some(0.08));
        assert_eq!(resolve_rate(None, Some(-1.0)), None);
        assert_eq!(resolve_rate(None, None), None);
        let mut p = PowerPlugin::new();
        p.set_kwh_rate(Some(0.08));
        assert_eq!(p.cfg_rate, Some(0.08));
    }

    #[test]
    fn amdgpu_fixture_only_counts_amd_cards() {
        let dir = crate::qa::harness::TempDir::new("power-amd");
        let root = dir.path();
        for (card, driver, uw) in [("card0", "amdgpu", "45000000"), ("card1", "i915", "999")] {
            let hw = root.join(card).join("device/hwmon/hwmon0");
            std::fs::create_dir_all(&hw).unwrap();
            std::fs::write(hw.join("power1_average"), uw).unwrap();
            std::os::unix::fs::symlink(
                format!("/sys/bus/pci/drivers/{driver}"),
                root.join(card).join("device/driver"),
            ).unwrap();
        }
        assert_eq!(amdgpu_watts(root), Some(45.0));
    }
}
