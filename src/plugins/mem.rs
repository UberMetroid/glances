//! Memory plugin — RAM total/used/free/available/percent.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::platform as plat;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "mem";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(MemPlugin::new()));
}

pub struct MemPlugin { base: GlancesPluginModel }

impl MemPlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        for k in &["total", "used", "free", "available", "percent", "active", "inactive", "buffers", "cached", "shared"] {
            m.insert(k.to_string(), Value::Float(0.0));
        }
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

impl Plugin for MemPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["percent"] }
    fn update(&mut self) -> Result<()> {
        use plat::linux::proc_meminfo as mi;
        let info = mi::read()?;
        let mut used = mi::used_mem(&info);
        let mut cached = info.cached;
        let mut available = info.available;
        let mut pct = mi::percent_used(&info);
        // ZFS ARC parity (upstream mem #3979): ARC counts as cached,
        // the shrinkable part counts as available (not used).
        if mi::zfs_enabled() {
            if let Some((size, cmin)) = mi::zfs_arc() {
                let shrink = size.saturating_sub(cmin);
                cached = cached.saturating_add(size);
                available = available.saturating_add(shrink);
                used = used.saturating_sub(shrink);
                if info.total > 0 {
                    pct = (info.total.saturating_sub(available) as f64 / info.total as f64) * 100.0;
                }
            }
        }
        // LXC/cgroup-v2 parity: `available` may exceed `total` — clamp
        // so used/percent never go negative or over 100.
        used = used.min(info.total);
        pct = pct.clamp(0.0, 100.0);
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("total".into(), Value::Float(info.total as f64));
            obj.insert("used".into(), Value::Float(used as f64));
            obj.insert("free".into(), Value::Float(mi::free_mem(&info) as f64));
            obj.insert("available".into(), Value::Float(available as f64));
            obj.insert("percent".into(), Value::Float(pct));
            obj.insert("active".into(), Value::Float(info.active as f64));
            obj.insert("inactive".into(), Value::Float(info.inactive as f64));
            obj.insert("buffers".into(), Value::Float(info.buffers as f64));
            obj.insert("cached".into(), Value::Float(cached as f64));
            obj.insert("shared".into(), Value::Float(info.shared as f64));
        }
        Ok(())
    }
    fn update_views(&mut self, events: &mut EventLog) {
        if let Some(m) = self.model_mut() {
            m.build_views(&[], None, None);
            let (used, total) = match m.stats.as_object() {
                Some(o) => (
                    o.get("used").and_then(Value::as_f64).unwrap_or(0.0),
                    o.get("total").and_then(Value::as_f64).unwrap_or(0.0),
                ),
                None => return,
            };
            if used > 0.0 && total > 0.0 {
                let d = m.get_alert_log(used, total, "", Some(&mut *events));
                m.views.entry(String::new()).or_default().insert("percent".into(), d);
            }
        }
    }
}
