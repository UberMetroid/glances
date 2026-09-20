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

/// `PRETTY_NAME` from /etc/os-release, e.g. "Ubuntu 24.04 LTS".
fn read_distro() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string()))
        })
        .unwrap_or_else(|| "Linux".to_string())
}

impl Plugin for SystemPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        let u = crate::platform::linux::uname::uname_info();
        // Hostname: kernel nodename, then /etc/hostname, then env.
        let hostname = u.as_ref().map(|i| i.nodename.clone()).filter(|s| !s.is_empty())
            .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| "localhost".into());
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("hostname".into(), Value::String(hostname));
            obj.insert("os_name".into(), Value::String(
                u.as_ref().map(|i| i.sysname.clone()).unwrap_or_else(|| std::env::consts::OS.to_string())));
            obj.insert("os_version".into(), Value::String(
                u.as_ref().map(|i| i.version.clone()).unwrap_or_default()));
            obj.insert("kernel".into(), Value::String(
                u.as_ref().map(|i| i.release.clone()).unwrap_or_default()));
            obj.insert("arch".into(), Value::String(
                u.as_ref().map(|i| i.machine.clone()).unwrap_or_else(|| std::env::consts::ARCH.to_string())));
            obj.insert("distro".into(), Value::String(read_distro()));
            obj.insert("platform".into(), Value::String(std::env::consts::FAMILY.to_string()));
        }
        Ok(())
    }
}
