//! System plugin — hostname, OS, kernel, arch, distro.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "system";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(SystemPlugin::new()));
}

pub struct SystemPlugin { base: GlancesPluginModel }

impl SystemPlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        m.insert("hostname".into(), Value::String(String::new()));
        m.insert("os_name".into(), Value::String(String::new()));
        m.insert("os_version".into(), Value::String(String::new()));
        m.insert("kernel".into(), Value::String(String::new()));
        m.insert("arch".into(), Value::String(String::new()));
        m.insert("distro".into(), Value::String(String::new()));
        m.insert("platform".into(), Value::String(String::new()));
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

impl Plugin for SystemPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        // Hostname from /etc/hostname on Linux; fall back to "HOSTNAME" env var.
        let hostname = std::fs::read_to_string("/etc/hostname")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".into()));
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("hostname".into(), Value::String(hostname));
            obj.insert("os_name".into(), Value::String(std::env::consts::OS.to_string()));
            obj.insert("os_version".into(), Value::String(std::env::consts::OS.to_string()));
            obj.insert("kernel".into(), Value::String("(std-only; no uname FFI)".into()));
            obj.insert("arch".into(), Value::String(std::env::consts::ARCH.to_string()));
            obj.insert("distro".into(), Value::String("(read /etc/os-release in M6-followup)".into()));
            obj.insert("platform".into(), Value::String(std::env::consts::FAMILY.to_string()));
        }
        Ok(())
    }
}
