//! Load-average plugin — 1/5/15-minute load via /proc/loadavg.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::platform as plat;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "load";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(LoadPlugin::new()));
}

pub struct LoadPlugin { base: GlancesPluginModel }

impl LoadPlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        for k in &["min1", "min5", "min15", "cpucore"] {
            m.insert(k.to_string(), Value::Float(0.0));
        }
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

impl Plugin for LoadPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["min1", "min5", "min15"] }
    fn update(&mut self) -> Result<()> {
        let l = plat::linux::proc_loadavg::read()?;
        // Machine-wide logical CPUs — `available_parallelism` reports
        // the process's sched affinity (cgroup-limited), not cpucore.
        let cores = plat::linux::proc_cpuinfo::cpu_count() as f64;
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("min1".into(), Value::Float(l.load1));
            obj.insert("min5".into(), Value::Float(l.load5));
            obj.insert("min15".into(), Value::Float(l.load15));
            obj.insert("cpucore".into(), Value::Float(cores));
        }
        Ok(())
    }
    fn update_views(&mut self, events: &mut EventLog) {
        if let Some(m) = self.model_mut() {
            m.build_views(&[], None, None);
            // Upstream load update_views: min15 alert+log, min5 alert
            // only; maximum scales with core count; missing keys (non-
            // Linux) skip silently.
            let (min5, min15, cores) = match m.stats.as_object() {
                Some(o) => (
                    o.get("min5").and_then(Value::as_f64),
                    o.get("min15").and_then(Value::as_f64),
                    o.get("cpucore").and_then(Value::as_f64).unwrap_or(1.0).max(1.0),
                ),
                None => return,
            };
            let d15 = min15.map(|v| m.get_alert_log(v, 100.0 * cores, "", Some(&mut *events)));
            let d5 = min5.map(|v| {
                m.get_alert(v, 0.0, 100.0 * cores, "", None, false, false, None, Some(&mut *events))
            });
            let views = m.views.entry(String::new()).or_default();
            if let Some(d) = d15 {
                views.insert("min15".into(), d);
            }
            if let Some(d) = d5 {
                views.insert("min5".into(), d);
            }
        }
    }
}
