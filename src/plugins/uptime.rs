//! Uptime plugin — seconds since boot.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::platform as plat;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "uptime";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(UptimePlugin::new()));
}

pub struct UptimePlugin { base: GlancesPluginModel }

impl UptimePlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        m.insert("seconds".into(), Value::Float(0.0));
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

impl Plugin for UptimePlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        let secs = plat::linux::proc_uptime::read_uptime()?;
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("seconds".into(), Value::Float(secs));
        }
        Ok(())
    }
}
