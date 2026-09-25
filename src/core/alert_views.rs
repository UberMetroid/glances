//! Decoration rebuild: one status word per field, refreshed each tick.
//!
//! Fields flagged ALERT classify through the alert engine, LOG fields
//! classify with logging, and everything else stays DEFAULT. List
//! plugins get a map per element; scalar plugins share one map.

use std::collections::HashMap;

use super::events::EventLog;
use super::plugin::{FieldDesc, FieldFlags, GlancesPluginModel};
use super::value::Value;

impl GlancesPluginModel {
    /// Rebuild every decoration from the current stats. `descs` is the
    /// plugin's field table and `key_field` its element identity (None
    /// for scalars). Stats are swapped aside during the rebuild so the
    /// classifier can borrow the model.
    pub fn build_views(
        &mut self,
        descs: &[FieldDesc],
        key_field: Option<&str>,
        events: Option<&mut EventLog>,
    ) {
        let stats = std::mem::replace(&mut self.stats, Value::Null);
        let mut log = events;
        let mut pass = |model: &mut Self, field: &str, v: &Value| {
            let n = v.as_f64().unwrap_or(0.0);
            model.classify_field(descs, field, n, log.as_deref_mut())
        };
        match &stats {
            Value::Array(items) => {
                let mut views = HashMap::new();
                for (i, item) in items.iter().enumerate() {
                    let Some(obj) = item.as_object() else { continue };
                    let mut fields = HashMap::new();
                    for (field, v) in obj {
                        fields.insert(field.clone(), pass(self, field, v));
                    }
                    views.insert(element_id(obj, key_field, i), fields);
                }
                self.views = views;
            }
            Value::Object(map) => {
                let mut fields = HashMap::new();
                for (field, v) in map {
                    fields.insert(field.clone(), pass(self, field, v));
                }
                self.views = HashMap::from([(String::new(), fields)]);
            }
            _ => {
                self.views = HashMap::new();
            }
        }
        self.stats = stats;
    }

    fn classify_field(
        &mut self,
        descs: &[FieldDesc],
        field: &str,
        value: f64,
        events: Option<&mut EventLog>,
    ) -> String {
        let flags = descs.iter().find(|d| d.name == field).map(|d| d.flags);
        if flags.is_some_and(|f| f.contains(FieldFlags::ALERT)) {
            self.get_alert(value, 0.0, 100.0, field, None, false, false, None, events)
        } else if flags.is_some_and(|f| f.contains(FieldFlags::LOG)) {
            self.get_alert_log(value, 100.0, field, events)
        } else {
            "DEFAULT".into()
        }
    }
}

/// Element identity: the key field as string/int/uint, else the index.
fn element_id(obj: &std::collections::BTreeMap<String, Value>, key_field: Option<&str>, index: usize) -> String {
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
