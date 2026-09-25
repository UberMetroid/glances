//! Uptime plugin — seconds since boot as one float.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "uptime";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(UptimePlugin::new()));
}

pub struct UptimePlugin { base: GlancesPluginModel }

impl Default for UptimePlugin {
    fn default() -> Self { Self::new() }
}

impl UptimePlugin {
    pub fn new() -> Self {
        let m = BTreeMap::from([("seconds".to_string(), Value::Float(0.0))]);
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
    fn update_snmp(&mut self, ctx: &crate::core::snmp::SnmpCtx) -> Result<()> {
        // sysUpTime.0 ticks in hundredths of a second.
        let m = crate::core::snmp::get_map(&ctx.client, &[("uptime", "1.3.6.1.2.1.1.3.0")])?;
        let secs = m.get("uptime").and_then(|v| v.as_f64()).unwrap_or(0.0) / 100.0;
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("seconds".into(), Value::Float(secs));
        }
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        let secs = plat::linux::proc_uptime::read_uptime()?;
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("seconds".into(), Value::Float(secs));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_reports_a_live_boot_clock() {
        let mut p = UptimePlugin::new();
        p.update().unwrap();
        let secs = p.stats().as_object().unwrap()["seconds"].as_f64().unwrap();
        assert!(secs.is_finite() && secs > 0.0);
    }
}
