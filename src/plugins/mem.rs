//! Memory plugin — RAM total/used/free/available/percent.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::platform as plat;
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
        for k in &["total", "used", "free", "available", "percent", "buffers", "cached", "shared"] {
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
    fn update(&mut self) -> Result<()> {
        let info = plat::linux::proc_meminfo::read()?;
        let used = plat::linux::proc_meminfo::used_mem(&info);
        let free = plat::linux::proc_meminfo::free_mem(&info);
        let pct = plat::linux::proc_meminfo::percent_used(&info);
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("total".into(), Value::Float(info.total as f64));
            obj.insert("used".into(), Value::Float(used as f64));
            obj.insert("free".into(), Value::Float(free as f64));
            obj.insert("available".into(), Value::Float(info.available as f64));
            obj.insert("percent".into(), Value::Float(pct));
            obj.insert("buffers".into(), Value::Float(info.buffers as f64));
            obj.insert("cached".into(), Value::Float(info.cached as f64));
            obj.insert("shared".into(), Value::Float(info.shared as f64));
        }
        Ok(())
    }
}
