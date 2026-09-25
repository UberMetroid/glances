//! S.M.A.R.T. disk health — per-device attributes via `smartctl`.
//!
//! Mirrors `glances/plugins/smart/__init__.py` (pySMART backend).
//! Linux-only. Each sweep runs `smartctl --scan` to enumerate devices
//! then `smartctl -a` per device, argv-only with no shell (same pattern
//! as `core/actions.rs`). Missing binary, missing permissions, or parse
//! failures yield an empty list — never an error. Sweeps are cached:
//! SMART values move slowly (health attributes rarely, temperature
//! over minutes), so one sweep per minute is plenty and per-tick
//! respawns would only burn forks on host installs.
//!
//! Stats are one object per device keyed by `DeviceName`
//! (`"<device> <model>"`): ATA devices carry an `attributes` table
//! (num/name/value/worst/threshold/type/raw), NVMe devices carry an
//! `nvme` health map parsed from log page 0x02.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

mod parse;

pub const NAME: &str = "smart";

/// Freshness window for a `smartctl` sweep.
const CACHE_TTL: Duration = Duration::from_secs(60);

pub use parse::{parse_attr_row, parse_device_output, parse_scan, SmartAttr, SmartDevice};
use parse::{run_smartctl, smartctl_bin};


pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(SmartPlugin::new()));
}

pub struct SmartPlugin {
    base: GlancesPluginModel,
    cached: Vec<SmartDevice>,
    collected_at: Option<Instant>,
}

impl Default for SmartPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl SmartPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            cached: Vec::new(),
            collected_at: None,
        }
    }
}

/// True when a sweep taken at `at` is still inside the TTL window
/// at `now`. Split out so the boundary is unit-testable.
pub fn cache_fresh(at: Option<Instant>, now: Instant) -> bool {
    at.is_some_and(|t| now.duration_since(t) < CACHE_TTL)
}

/// Enumerate devices and read their attributes. Empty on any failure.
pub fn collect() -> Vec<SmartDevice> {
    let bin = match smartctl_bin() {
        Some(b) => b,
        None => return Vec::new(),
    };
    let scan = match run_smartctl(&bin, &["--scan"]) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for (device, typ) in parse_scan(&scan) {
        let text = match run_smartctl(&bin, &["-a", "-d", &typ, &device]) {
            Some(t) => t,
            None => continue,
        };
        out.push(parse_device_output(&device, &typ, &text));
    }
    out
}

/// Render one device as a stats object keyed by `DeviceName`.
pub fn device_to_value(d: &SmartDevice) -> Value {
    let mut obj = BTreeMap::new();
    let name = if d.model.is_empty() {
        d.device.clone()
    } else {
        format!("{} {}", d.device, d.model)
    };
    obj.insert("DeviceName".into(), Value::String(name));
    obj.insert("model".into(), Value::String(d.model.clone()));
    obj.insert("serial".into(), Value::String(d.serial.clone()));
    obj.insert("protocol".into(), Value::String(d.protocol.clone()));
    obj.insert(
        "attributes".into(),
        Value::Array(
            d.attributes
                .iter()
                .map(|a| {
                    let mut m = BTreeMap::new();
                    m.insert("num".into(), Value::Uint(a.num));
                    m.insert("name".into(), Value::String(a.name.clone()));
                    m.insert("value".into(), Value::Uint(a.value));
                    m.insert("worst".into(), Value::Uint(a.worst));
                    m.insert("threshold".into(), Value::Uint(a.threshold));
                    m.insert("type".into(), Value::String(a.attr_type.clone()));
                    m.insert("raw".into(), Value::String(a.raw.clone()));
                    Value::Object(m)
                })
                .collect(),
        ),
    );
    obj.insert(
        "nvme".into(),
        Value::Array(
            d.nvme
                .iter()
                .map(|(k, v)| {
                    let mut m = BTreeMap::new();
                    m.insert("name".into(), Value::String(k.clone()));
                    m.insert("value".into(), Value::String(v.clone()));
                    Value::Object(m)
                })
                .collect(),
        ),
    );
    Value::Object(obj)
}

impl Plugin for SmartPlugin {
    fn name(&self) -> &'static str {
        NAME
    }
    fn reset(&mut self) {
        self.base.reset();
        self.cached.clear();
        self.collected_at = None;
    }
    fn stats(&self) -> &Value {
        &self.base.stats
    }
    fn model(&self) -> Option<&GlancesPluginModel> {
        Some(&self.base)
    }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> {
        Some(&mut self.base)
    }
    fn stats_mut(&mut self) -> &mut Value {
        &mut self.base.stats
    }
    fn get_key(&self) -> Option<&'static str> {
        Some("DeviceName")
    }

    fn update(&mut self) -> Result<()> {
        if !cfg!(target_os = "linux") {
            self.base.stats = Value::Array(Vec::new());
            return Ok(());
        }
        let now = Instant::now();
        if !cache_fresh(self.collected_at, now) {
            self.cached = collect();
            self.collected_at = Some(now);
        }
        self.base.stats = Value::Array(self.cached.iter().map(device_to_value).collect());
        Ok(())
    }
}
