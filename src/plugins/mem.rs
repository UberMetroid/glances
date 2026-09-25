//! Memory plugin — RAM total/used/free/available/percent + details.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "mem";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(MemPlugin::new()));
}

const FIELDS: &[&str] =
    &["total", "used", "free", "available", "percent", "active", "inactive", "buffers", "cached", "shared"];

pub struct MemPlugin { base: GlancesPluginModel }

impl Default for MemPlugin {
    fn default() -> Self { Self::new() }
}

impl MemPlugin {
    pub fn new() -> Self {
        let m: BTreeMap<String, Value> =
            FIELDS.iter().map(|k| (k.to_string(), Value::Float(0.0))).collect();
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
    fn update_snmp(&mut self, ctx: &crate::core::snmp::SnmpCtx) -> Result<()> {
        // UCD memory MIB, kilobytes → bytes.
        let m = crate::core::snmp::get_map(&ctx.client, &[
            ("total", "1.3.6.1.4.1.2021.4.5.0"),
            ("free", "1.3.6.1.4.1.2021.4.11.0"),
            ("shared", "1.3.6.1.4.1.2021.4.13.0"),
            ("buffers", "1.3.6.1.4.1.2021.4.14.0"),
            ("cached", "1.3.6.1.4.1.2021.4.15.0"),
        ])?;
        let kb = |k: &str| m.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) * 1024.0;
        let (total, free) = (kb("total"), kb("free"));
        if total <= 0.0 {
            self.reset();
            return Ok(());
        }
        let used = (total - free).max(0.0);
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("total".into(), Value::Float(total));
            obj.insert("used".into(), Value::Float(used));
            obj.insert("free".into(), Value::Float(free));
            obj.insert("available".into(), Value::Float(free));
            obj.insert("percent".into(), Value::Float((used / total * 100.0).clamp(0.0, 100.0)));
            obj.insert("shared".into(), Value::Float(kb("shared")));
            obj.insert("buffers".into(), Value::Float(kb("buffers")));
            obj.insert("cached".into(), Value::Float(kb("cached")));
        }
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        use plat::linux::proc_meminfo as mi;
        let info = mi::read()?;
        let mut used = mi::used_mem(&info);
        let mut cached = info.cached;
        let mut available = info.available;
        let mut pct = mi::percent_used(&info);
        // ZFS ARC: ARC counts as cached, and its shrinkable part counts
        // as available (reclaimable) rather than used.
        if mi::zfs_enabled()
            && let Some((size, cmin)) = mi::zfs_arc() {
                let shrink = size.saturating_sub(cmin);
                cached = cached.saturating_add(size);
                available = available.saturating_add(shrink);
                used = used.saturating_sub(shrink);
                if info.total > 0 {
                    pct = (info.total.saturating_sub(available) as f64 / info.total as f64) * 100.0;
                }
            }
        // LXC/cgroup-v2: `available` may exceed `total` — clamp so
        // used/percent never go negative or past 100.
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
        let Some(m) = self.model_mut() else { return };
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_box_classifies_nothing() {
        let mut p = MemPlugin::new();
        let mut log = EventLog::default();
        p.update_views(&mut log);
        assert_eq!(
            p.base.views[&String::new()].get("percent").map(String::as_str),
            Some("DEFAULT")
        );
        assert!(log.is_empty());
    }
}
