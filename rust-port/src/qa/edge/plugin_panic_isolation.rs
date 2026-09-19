//! AC-12 edge case: a panicking plugin must not crash the refresh loop.

use std::sync::Arc;

use crate::core::error::Result;
use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;

struct PanicPlugin;
impl Plugin for PanicPlugin {
    fn name(&self) -> &'static str { "panic" }
    fn reset(&mut self) {}
    fn update(&mut self) -> Result<()> { panic!("simulated panic in plugin"); }
    fn stats(&self) -> &Value { &Value::Null }
    fn stats_mut(&mut self) -> &mut Value { unimplemented!() }
}

struct OkPlugin;
impl Plugin for OkPlugin {
    fn name(&self) -> &'static str { "ok" }
    fn reset(&mut self) {}
    fn update(&mut self) -> Result<()> { Ok(()) }
    fn stats(&self) -> &Value { &Value::Null }
    fn stats_mut(&mut self) -> &mut Value { unimplemented!() }
}

#[test]
fn panic_in_one_plugin_does_not_kill_loop() {
    let stats = Arc::new(GlancesStats::new(2.0));
    stats.register(Box::new(OkPlugin));
    stats.register(Box::new(PanicPlugin));
    stats.register(Box::new(OkPlugin));

    // First update: panic plugin panics, ok plugins succeed, loop returns Ok.
    stats.update().expect("update should not propagate panic");
    // Second update: same — should still work (state preserved).
    stats.update().expect("second update should also work");
}
