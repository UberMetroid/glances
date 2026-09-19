//! Plugin trait + base struct mirroring GlancesPluginModel (model.py:56).
//!
//! See `docs/ARCHITECTURE.md` §3.2 for the full design rationale.
//!
//! M1 implementation: trait, base struct with default impls. Plugin
//! subclasses in `src/plugins/*.rs` will fill in `update()` etc.

use std::collections::HashMap;

use super::error::Result;
use super::history::GlancesHistory;
use super::timer::Timer;
use super::value::Value;

/// Per-field metadata. Mirrors Python Glances' `fields_description`.
#[derive(Debug, Clone)]
pub struct FieldDesc {
    pub name: &'static str,
    pub unit: Unit,
    pub flags: FieldFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Percent,
    Bytes,
    Number,
    Second,
    Float,
    String,
    Bool,
}

/// Bit flags describing field behavior (rate, min/max/mean, log, alert, optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldFlags(pub u8);

impl FieldFlags {
    pub const RATE:     Self = Self(0b0000_0001);
    pub const MMM:      Self = Self(0b0000_0010);
    pub const LOG:      Self = Self(0b0000_0100);
    pub const ALERT:    Self = Self(0b0000_1000);
    pub const OPTIONAL: Self = Self(0b0001_0000);

    pub fn empty() -> Self { Self(0) }
    pub fn contains(self, other: Self) -> bool { (self.0 & other.0) == other.0 }
}

impl std::ops::BitOr for FieldFlags {
    type Output = Self;
    fn bitor(self, other: Self) -> Self { Self(self.0 | other.0) }
}

/// Trait every plugin must implement. The base struct `GlancesPluginModel`
/// provides default impls; plugins override only what they need.
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn reset(&mut self);
    fn update(&mut self) -> Result<()>;
    fn stats(&self) -> &Value;
    fn stats_mut(&mut self) -> &mut Value;
    fn get_key(&self) -> Option<&'static str> { None }
    fn fields_description(&self) -> &[FieldDesc] { &[] }
    fn update_views(&mut self) {}
    fn update_stats_history(&mut self) {}
    fn exit(&mut self) {}
    fn is_enabled(&self) -> bool { true }
}

/// Base struct providing the field/state every plugin has. Mirrors
/// `glances/plugins/plugin/model.py` lines 84-145.
pub struct GlancesPluginModel {
    pub plugin_name: &'static str,
    pub stats: Value,
    pub stats_init_value: Value,
    pub refresh_timer: Timer,
    pub stats_history: GlancesHistory,
    pub limits: HashMap<String, LimitValue>,
}

impl GlancesPluginModel {
    pub fn new(plugin_name: &'static str, stats_init_value: Value) -> Self {
        Self {
            plugin_name,
            stats: stats_init_value.clone(),
            stats_init_value,
            refresh_timer: Timer::new(0.0),
            stats_history: GlancesHistory::new(),
            limits: HashMap::new(),
        }
    }

    /// Default `reset()` — mirrors `model.py:301`.
    pub fn reset(&mut self) {
        self.stats = self.stats_init_value.clone();
    }
}

/// Limit value: either a single float (e.g. `cpu_user_careful = 50`)
/// or a list of strings (e.g. `cpu_user_show = "core0,core1"`).
#[derive(Debug, Clone)]
pub enum LimitValue {
    Float(f64),
    List(Vec<String>),
}
