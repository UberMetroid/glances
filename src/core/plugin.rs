//! Plugin trait + base struct mirroring GlancesPluginModel (model.py:56).

use std::collections::HashMap;

use super::alerts::LimitValue;
use super::error::Result;
use super::history::GlancesHistory;
use super::timer::Timer;
use super::value::Value;

#[derive(Debug, Clone)]
pub struct FieldDesc {
    pub name: &'static str,
    pub unit: Unit,
    pub flags: FieldFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Percent, Bytes, Number, Second, Float, String, Bool,
}

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

pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn reset(&mut self);
    fn update(&mut self) -> Result<()>;
    fn stats(&self) -> &Value;
    fn stats_mut(&mut self) -> &mut Value;
    /// Access the shared model (limits, history, timers). All in-tree
    /// plugins wrap `GlancesPluginModel`; `None` is for exotic impls.
    fn model(&self) -> Option<&GlancesPluginModel> { None }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { None }
    fn get_key(&self) -> Option<&'static str> { None }
    fn fields_description(&self) -> &[FieldDesc] { &[] }
    /// Curated history series for this plugin (upstream
    /// `items_history_list` parity: field names only).
    fn history_items(&self) -> &[&'static str] { &[] }
    /// Replace the display filter (upstream `process_filter` setter).
    /// Only processlist honors it; the default is a no-op.
    fn set_process_filter(&mut self, _raw: Option<&str>) {}
    /// Rebuild alert decorations into the model views (upstream
    /// `update_views` parity). Base implementation decorates every
    /// field; plugins with per-stat rules override it.
    fn update_views(&mut self, events: &mut super::events::EventLog) {
        let key = self.get_key();
        let descs: Vec<FieldDesc> = self.fields_description().to_vec();
        if let Some(m) = self.model_mut() {
            m.build_views(&descs, key, Some(events));
        }
    }
    /// Record this plugin's curated history series (upstream
    /// `update_stats_history` parity). No-op when the plugin declares
    /// no `history_items`.
    fn update_stats_history(&mut self) {
        let items: Vec<&'static str> = self.history_items().to_vec();
        if items.is_empty() {
            return;
        }
        let key = self.get_key();
        if let Some(m) = self.model_mut() {
            let size = m
                .limits
                .get("history_size")
                .and_then(|v| match v {
                    LimitValue::Float(f) => Some(*f as usize),
                    LimitValue::List(l) => l.first()?.parse::<usize>().ok(),
                })
                .unwrap_or(28800);
            m.update_stats_history(&items, key, size);
        }
    }
    fn exit(&mut self) {}
    fn is_enabled(&self) -> bool { true }
}

pub struct GlancesPluginModel {
    pub plugin_name: &'static str,
    pub stats: Value,
    pub stats_init_value: Value,
    pub refresh_timer: Timer,
    pub stats_history: GlancesHistory,
    pub limits: HashMap<String, LimitValue>,
    /// Alert decorations: element id → field → decoration string
    /// (upstream `views` parity; `""` element for scalar plugins).
    pub views: HashMap<String, HashMap<String, String>>,
    /// Last trigger per stat (`manage_threshold` parity).
    pub thresholds: HashMap<String, String>,
    pub prev_stats: Option<Value>,
    pub prev_time: Option<std::time::Instant>,
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

    /// Compute per-second rates for fields marked RATE.
    /// Adds `<key>_gauge`, `<key>_rate_per_sec`, and `time_since_update` siblings.
    pub fn manage_rate(&mut self) {
        let now = std::time::Instant::now();
        let dt = match self.prev_time {
            Some(t) => now.duration_since(t).as_secs_f64(),
            None => { self.prev_stats = Some(self.stats.clone()); self.prev_time = Some(now); return; }
        };
        if dt <= 0.0 { return; }
        let prev_map = match self.prev_stats.as_ref().and_then(|v| v.as_object()) {
            Some(m) => m.clone(),
            None => { self.prev_stats = Some(self.stats.clone()); self.prev_time = Some(now); return; }
        };
        // Collect keys first (to avoid borrow issues).
        let keys: Vec<String> = match self.stats.as_object() {
            Some(o) => o.keys().cloned().collect(),
            None => return,
        };
        for k in keys {
            if k.ends_with("_gauge") || k.ends_with("_rate_per_sec") || k == "time_since_update" { continue; }
            let cur_v = self.stats.as_object().and_then(|o| o.get(&k)).and_then(Value::as_f64);
            let prev_v = prev_map.get(&k).and_then(Value::as_f64);
            if let (Some(c), Some(p)) = (cur_v, prev_v) {
                if let Some(obj) = self.stats.as_object_mut() {
                    obj.insert(format!("{}_gauge", k), Value::Float(c));
                    obj.insert(format!("{}_rate_per_sec", k), Value::Float((c - p) / dt));
                }
            }
        }
        if let Some(obj) = self.stats.as_object_mut() {
            obj.insert("time_since_update".into(), Value::Float(dt));
        }
        self.prev_stats = Some(self.stats.clone());
        self.prev_time = Some(now);
    }

    /// Track min/max/mean for fields marked MMM.
    pub fn manage_mmm(&mut self) {
        let keys: Vec<String> = match self.stats.as_object() {
            Some(o) => o.keys().cloned().collect(),
            None => return,
        };
        for k in &keys {
            if k.ends_with("_min") || k.ends_with("_max") || k.ends_with("_mean") { continue; }
            let v = self.stats.as_object().and_then(|o| o.get(k)).and_then(Value::as_f64);
            if let Some(f) = v {
                let entry = self.mmm_buffer.entry(k.clone()).or_insert((f, f, 0.0, 0));
                if f < entry.0 { entry.0 = f; }
                if f > entry.1 { entry.1 = f; }
                entry.2 += f;
                entry.3 += 1;
            }
        }
        for (k, (min, max, sum, count)) in self.mmm_buffer.iter() {
            if let Some(obj) = self.stats.as_object_mut() {
                obj.insert(format!("{}_min", k), Value::Float(*min));
                obj.insert(format!("{}_max", k), Value::Float(*max));
                obj.insert(format!("{}_mean", k), Value::Float(sum / *count as f64));
            }
        }
    }

    pub fn update_stats_history_for(&mut self, fields: &[&'static str]) {
        for field in fields {
            if let Some(v) = self.stats.as_object().and_then(|o| o.get(*field)).and_then(Value::as_f64) {
                self.stats_history.add(field, v);
            }
        }
    }

    /// Record curated history series (upstream `update_stats_history`
    /// parity): scalar stats record `<field>`; list stats record one
    /// `<elem>_<field>` series per element (element identity from
    /// `key_field`, index fallback). Missing fields are skipped.
    pub fn update_stats_history(
        &mut self,
        items: &[&'static str],
        key_field: Option<&str>,
        history_size: usize,
    ) {
        self.stats_history.set_max_size(history_size);
        match self.stats.clone() {
            Value::Array(elems) => {
                for (i, elem) in elems.iter().enumerate() {
                    let obj = match elem.as_object() {
                        Some(o) => o,
                        None => continue,
                    };
                    let id = key_field
                        .and_then(|kf| obj.get(kf))
                        .and_then(|v| match v {
                            Value::String(s) if !s.is_empty() => Some(s.clone()),
                            Value::Int(n) => Some(n.to_string()),
                            Value::Uint(n) => Some(n.to_string()),
                            _ => None,
                        })
                        .unwrap_or_else(|| i.to_string());
                    for field in items {
                        if let Some(v) = obj.get(*field).and_then(Value::as_f64) {
                            self.stats_history.add(&format!("{}_{}", id, field), v);
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
