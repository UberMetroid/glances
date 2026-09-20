//! Swap memory plugin — total/used/free/percent.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::platform as plat;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "memswap";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(MemswapPlugin::new()));
}

pub struct MemswapPlugin { base: GlancesPluginModel }

impl MemswapPlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        for k in &["total", "used", "free", "percent"] {
            m.insert(k.to_string(), Value::Float(0.0));
        }
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

impl Plugin for MemswapPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        let info = plat::linux::proc_meminfo::read()?;
        let used = info.swap_total.saturating_sub(info.swap_free);
        let pct = if info.swap_total > 0 { (used as f64 / info.swap_total as f64) * 100.0 } else { 0.0 };
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("total".into(), Value::Float(info.swap_total as f64));
            obj.insert("used".into(), Value::Float(used as f64));
            obj.insert("free".into(), Value::Float(info.swap_free as f64));
            obj.insert("percent".into(), Value::Float(pct));
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
            if total > 0.0 {
                let d = m.get_alert_log(used, total, "", Some(&mut *events));
                m.views.entry(String::new()).or_default().insert("percent".into(), d);
            }
        }
    }
}
