//! Plugin framework: the data-source trait and its shared model.
//!
//! Every plugin implements `Plugin` (identity, refresh, stats access)
//! and embeds `GlancesPluginModel` (stats storage, limits, history,
//! alert decorations, rate and min/max/mean tracking).

use std::collections::HashMap;

use super::alerts::LimitValue;
use super::error::Result;
use super::history::GlancesHistory;
use super::timer::Timer;
use super::value::Value;

/// One documented stat: its name, unit, and behavior flags.
#[derive(Debug, Clone)]
pub struct FieldDesc {
    pub name: &'static str,
    pub unit: Unit,
    pub flags: FieldFlags,
}

/// Display unit for a stat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Percent, Bytes, Number, Second, Float, String, Bool,
}

/// Behavior flags for a stat: rate tracking, min/max/mean, alert
/// classification, alert logging, optional (may be absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldFlags(pub u8);

impl FieldFlags {
    pub const RATE: Self = Self(0b0000_0001);
    pub const MMM: Self = Self(0b0000_0010);
    pub const LOG: Self = Self(0b0000_0100);
    pub const ALERT: Self = Self(0b0000_1000);
    pub const OPTIONAL: Self = Self(0b0001_0000);

    pub fn empty() -> Self { Self(0) }
    pub fn contains(self, other: Self) -> bool { self.0 & other.0 == other.0 }
}

impl std::ops::BitOr for FieldFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self { Self(self.0 | rhs.0) }
}

/// A data source. The refresh loop calls `update`, then history,
/// actions, and `update_views`; readers use `stats`.
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn reset(&mut self);
    fn update(&mut self) -> Result<()>;
    /// SNMP client-mode refresh. Only MIB-backed plugins implement it;
    /// the default reports Unsupported and keeps stale stats.
    fn update_snmp(&mut self, ctx: &super::snmp::SnmpCtx) -> Result<()> {
        let _ = ctx;
        Err(super::error::GlancesError::Unsupported(self.name().to_string()))
    }
    fn stats(&self) -> &Value;
    fn stats_mut(&mut self) -> &mut Value;
    /// The shared model, when the plugin embeds one.
    fn model(&self) -> Option<&GlancesPluginModel> { None }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { None }
    /// Element identity field for list stats (None for scalars).
    fn get_key(&self) -> Option<&'static str> { None }
    fn fields_description(&self) -> &[FieldDesc] { &[] }
    /// Series names recorded into history each tick.
    fn history_items(&self) -> &[&'static str] { &[] }
    /// Startup setters; only the named plugin honors each one.
    fn set_process_filter(&mut self, _raw: Option<&str>) {}
    fn set_irix_divide(&mut self, _divide: bool) {}
    fn set_kwh_rate(&mut self, _rate: Option<f64>) {}
    /// Rebuild alert decorations from the current stats.
    fn update_views(&mut self, events: &mut super::events::EventLog) {
        let descs: Vec<FieldDesc> = self.fields_description().to_vec();
        let key = self.get_key();
        if let Some(m) = self.model_mut() {
            m.build_views(&descs, key, Some(events));
        }
    }
    /// Record this tick's curated history series (no-op without
    /// `history_items`).
    fn update_stats_history(&mut self) {
        let items: Vec<&'static str> = self.history_items().to_vec();
        if items.is_empty() {
            return;
        }
        let key = self.get_key();
        if let Some(m) = self.model_mut() {
            m.update_stats_history(&items, key, history_size_of(m));
        }
    }
    fn exit(&mut self) {}
    fn is_enabled(&self) -> bool { true }
}

fn history_size_of(m: &GlancesPluginModel) -> usize {
    m.limits.get("history_size").and_then(|v| match v {
        LimitValue::Float(f) => Some(*f as usize),
        LimitValue::List(l) => l.first()?.parse::<usize>().ok(),
    }).unwrap_or(28800)
}

/// Shared per-plugin state: stats, limits, history, decorations,
/// triggers, and the previous tick (for rates and deltas).
pub struct GlancesPluginModel {
    pub plugin_name: &'static str,
    pub stats: Value,
    pub stats_init_value: Value,
    pub refresh_timer: Timer,
    pub stats_history: GlancesHistory,
    pub limits: HashMap<String, LimitValue>,
    /// Decorations: element id → field → status word ("" id for scalars).
    pub views: HashMap<String, HashMap<String, String>>,
    /// Last trigger word per stat.
    pub thresholds: HashMap<String, String>,
    pub prev_stats: Option<Value>,
    pub prev_time: Option<std::time::Instant>,
    /// Min/max/sum/count per field for MMM tracking.
    pub mmm_buffer: HashMap<String, (f64, f64, f64, u64)>,
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
            views: HashMap::new(),
            thresholds: HashMap::new(),
            prev_stats: None,
            prev_time: None,
            mmm_buffer: HashMap::new(),
        }
    }

    pub fn reset(&mut self) { self.stats = self.stats_init_value.clone(); }

    /// Per-second rates for counter fields. Adds `<key>_gauge`,
    /// `<key>_rate_per_sec`, and `time_since_update`; derived keys are
    /// never re-derived. The first tick only primes the baseline.
    pub fn manage_rate(&mut self) {
        let now = std::time::Instant::now();
        let dt = self.prev_time.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        let prev_obj = self.prev_stats.as_ref().and_then(|v| v.as_object()).is_some();
        if dt <= 0.0 || !prev_obj {
            self.prev_stats = Some(self.stats.clone());
            self.prev_time = Some(now);
            return;
        }
        let keys: Vec<String> = self.stats.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        for k in keys {
            if k.ends_with("_gauge") || k.ends_with("_rate_per_sec") || k == "time_since_update" {
                continue;
            }
            let cur = self.stats.as_object().and_then(|o| o.get(&k)).and_then(Value::as_f64);
            let old = self.prev_stats.as_ref().and_then(|v| v.as_object()).and_then(|m| m.get(&k)).and_then(Value::as_f64);
            if let (Some(c), Some(p), Some(obj)) = (cur, old, self.stats.as_object_mut()) {
                obj.insert(format!("{k}_gauge"), Value::Float(c));
                obj.insert(format!("{k}_rate_per_sec"), Value::Float((c - p) / dt));
            }
        }
        if let Some(obj) = self.stats.as_object_mut() {
            obj.insert("time_since_update".into(), Value::Float(dt));
        }
        self.prev_stats = Some(self.stats.clone());
        self.prev_time = Some(now);
    }

    /// Running min/max/mean per numeric field, emitted as
    /// `<key>_min`, `<key>_max`, `<key>_mean`.
    pub fn manage_mmm(&mut self) {
        let keys: Vec<String> = self.stats.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        for k in &keys {
            if k.ends_with("_min") || k.ends_with("_max") || k.ends_with("_mean") {
                continue;
            }
            if let Some(f) = self.stats.as_object().and_then(|o| o.get(k)).and_then(Value::as_f64) {
                let slot = self.mmm_buffer.entry(k.clone()).or_insert((f, f, 0.0, 0));
                slot.0 = slot.0.min(f);
                slot.1 = slot.1.max(f);
                slot.2 += f;
                slot.3 += 1;
            }
        }
        for (k, (lo, hi, sum, n)) in self.mmm_buffer.iter() {
            if let Some(obj) = self.stats.as_object_mut() {
                obj.insert(format!("{k}_min"), Value::Float(*lo));
                obj.insert(format!("{k}_max"), Value::Float(*hi));
                obj.insert(format!("{k}_mean"), Value::Float(sum / *n as f64));
            }
        }
    }

    /// Record scalar fields straight into history.
    pub fn update_stats_history_for(&mut self, fields: &[&'static str]) {
        for field in fields {
            if let Some(v) = self.stats.as_object().and_then(|o| o.get(*field)).and_then(Value::as_f64) {
                self.stats_history.add(field, v);
            }
        }
    }

    /// Record the curated series: `<field>` for scalar stats,
    /// `<element>_<field>` for list stats (element id from `key_field`,
    /// index fallback). Missing fields are skipped.
    pub fn update_stats_history(
        &mut self,
        items: &[&'static str],
        key_field: Option<&str>,
        history_size: usize,
    ) {
        self.stats_history.set_max_size(history_size);
        match &self.stats {
            Value::Array(elems) => {
                for (i, elem) in elems.iter().enumerate() {
                    let Some(obj) = elem.as_object() else { continue };
                    let id = element_label(obj, key_field, i);
                    for field in items {
                        if let Some(v) = obj.get(*field).and_then(Value::as_f64) {
                            self.stats_history.add(&format!("{id}_{field}"), v);
                        }
                    }
                }
            }
            Value::Object(map) => {
                for field in items {
                    if let Some(v) = map.get(*field).and_then(Value::as_f64) {
                        self.stats_history.add(field, v);
                    }
                }
            }
            _ => {}
        }
    }
}

fn element_label(obj: &std::collections::BTreeMap<String, Value>, key_field: Option<&str>, index: usize) -> String {
    key_field
        .and_then(|kf| obj.get(kf))
        .and_then(|v| match v {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Int(n) => Some(n.to_string()),
            Value::Uint(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_else(|| index.to_string())
}
