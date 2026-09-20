//! AMP (Application Monitoring Process) plugin.
//!
//! Mirrors `glances/plugins/amp/__init__.py`. Each AMP runs a configured
//! command (e.g. `ps`, `netstat`, `nginx -V`) and matches its output
//! against the running process list. The matched processes are exposed
//! under their AMP name.
//!
//! ## M11 status: STUB
//!
//! The full AMP wiring depends on M12:
//!
//! - Config-driven AMP list (`glances.conf [amp_*]` sections are
//!   currently parsed by `crate::core::config::Config` but the
//!   `[amp_*]` iteration is not wired into this plugin yet).
//! - Per-AMP argv-only subprocess runner via `crate::exec`
//!   (today `crate::exec` is a thin re-export of `std::process::Command`
//!   — M12 will introduce a hardened `safe_run.rs` with allow-lists
//!   and env scrubbing per plan §3.5 hard constraint #4).
//! - Regex matching against the process list (needs `processes` plugin).
//!
//! For M11 we register the plugin and always emit an empty array. The
//! name, key, and shape match the contract so M12 can fill in the
//! internals without changing the wire format.

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
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value {
        &mut self.base.stats
    }
    fn get_key(&self) -> Option<&'static str> {
        Some("name")
    }

    fn update(&mut self) -> Result<()> {
        // M11 stub: leave the stats as an empty array. M12 will:
        //   1. Read [amp_*] sections from the loaded Config.
        //   2. For each AMP, argv-spawn its command via crate::exec.
        //   3. Parse the command output and match against /proc/*/cmdline.
        //   4. Push a {name, count, processes[]} row per AMP.
        self.base.stats = Value::Array(Vec::new());
        Ok(())
    }
}
