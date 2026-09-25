//! Load-average plugin — 1/5/15-minute load plus core count.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "load";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(LoadPlugin::new()));
}

pub struct LoadPlugin { base: GlancesPluginModel }

impl Default for LoadPlugin {
    fn default() -> Self { Self::new() }
}

impl LoadPlugin {
    pub fn new() -> Self {
        let m: BTreeMap<String, Value> = ["min1", "min5", "min15", "cpucore"]
            .into_iter()
            .map(|k| (k.to_string(), Value::Float(0.0)))
            .collect();
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

/// Machine-wide logical CPUs. (The process-parallelism count reports
/// sched affinity — cgroup-limited — instead of the machine total.)
fn core_count() -> f64 {
    plat::linux::proc_cpuinfo::cpu_count() as f64
}

impl Plugin for LoadPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["min1", "min5", "min15"] }
    fn update_snmp(&mut self, ctx: &crate::core::snmp::SnmpCtx) -> Result<()> {
        // UCD load strings; core count stays local (no MIB for it).
        let m = crate::core::snmp::get_map(&ctx.client, &[
            ("min1", "1.3.6.1.4.1.2021.10.1.3.1"),
            ("min5", "1.3.6.1.4.1.2021.10.1.3.2"),
            ("min15", "1.3.6.1.4.1.2021.10.1.3.3"),
        ])?;
        if let Some(obj) = self.base.stats.as_object_mut() {
            for k in ["min1", "min5", "min15"] {
                obj.insert(k.into(), Value::Float(m.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0)));
            }
            obj.insert("cpucore".into(), Value::Float(core_count()));
        }
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        let l = plat::linux::proc_loadavg::read()?;
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("min1".into(), Value::Float(l.load1));
            obj.insert("min5".into(), Value::Float(l.load5));
            obj.insert("min15".into(), Value::Float(l.load15));
            obj.insert("cpucore".into(), Value::Float(core_count()));
        }
        Ok(())
    }
    fn update_views(&mut self, events: &mut EventLog) {
        let Some(m) = self.model_mut() else { return };
        m.build_views(&[], None, None);
        // min15 classifies with logging, min5 without; the maximum
        // scales with core count; missing keys skip silently.
        let (min5, min15, cores) = match m.stats.as_object() {
            Some(o) => (
                o.get("min5").and_then(Value::as_f64),
                o.get("min15").and_then(Value::as_f64),
                o.get("cpucore").and_then(Value::as_f64).unwrap_or(1.0).max(1.0),
            ),
            None => return,
        };
        let span = 100.0 * cores;
        if let Some(v) = min15 {
            let d = m.get_alert_log(v, span, "", Some(&mut *events));
            m.views.entry(String::new()).or_default().insert("min15".into(), d);
        }
        if let Some(v) = min5 {
            let d = m.get_alert(v, 0.0, span, "", None, false, false, None, Some(&mut *events));
            m.views.entry(String::new()).or_default().insert("min5".into(), d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn views_scale_with_core_count() {
        let mut p = LoadPlugin::new();
        // 8-core box at load 8: min15 hits 100% of span.
        if let Some(o) = p.stats_mut().as_object_mut() {
            o.insert("min5".into(), Value::Float(8.0));
            o.insert("min15".into(), Value::Float(8.0));
            o.insert("cpucore".into(), Value::Float(8.0));
        }
        let mut log = EventLog::default();
        p.update_views(&mut log);
        let views = &p.base.views[&String::new()];
        assert!(views.contains_key("min15") && views.contains_key("min5"));
    }
}
