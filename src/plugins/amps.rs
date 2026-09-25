//! AMP (Application Monitoring Process) plugin.
//!
//! Each AMP runs a configured command and matches its output against
//! the running process list, exposing the matches under the AMP name.
//!
//! ## Current status: STUB
//!
//! The full wiring is not built yet:
//!
//! - Config-driven AMP list (`[amp_*]` sections) is not iterated here.
//! - Per-AMP argv-only subprocess runner with allow-lists and env
//!   scrubbing does not exist yet.
//! - Regex matching against the process list needs the `processes`
//!   plugin surface.
//!
//! Until then the plugin registers and always emits an empty array.
//! Name, key, and shape match the contract so the internals can be
//! filled in without changing the wire format.

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "amps";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(AmpsPlugin::new()));
}

pub struct AmpsPlugin {
    base: GlancesPluginModel,
}

impl AmpsPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
        }
    }
}

impl Default for AmpsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for AmpsPlugin {
    fn name(&self) -> &'static str {
        NAME
    }
    fn reset(&mut self) {
        self.base.reset();
    }
    fn stats(&self) -> &Value {
        &self.base.stats
    }
    fn model(&self) -> Option<&GlancesPluginModel> {
        Some(&self.base)
    }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> {
        Some(&mut self.base)
    }
    fn stats_mut(&mut self) -> &mut Value {
        &mut self.base.stats
    }
    fn get_key(&self) -> Option<&'static str> {
        Some("name")
    }

    fn update(&mut self) -> Result<()> {
        // Stub: no AMP sources configured yet, so there is nothing to
        // match. A future version will, per tick:
        //   1. Read [amp_*] sections from the loaded Config.
        //   2. Argv-spawn each AMP command via the hardened runner.
        //   3. Parse the output and match against /proc/*/cmdline.
        //   4. Push a {name, count, processes[]} row per AMP.
        self.base.stats = Value::Array(Vec::new());
        Ok(())
    }
}
