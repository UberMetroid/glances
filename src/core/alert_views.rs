//! Alert views — base update_views rebuild and field decoration.
//! Upstream GlancesPluginModel.update_views parity.

use std::collections::HashMap;

use super::events::EventLog;
use super::plugin::{FieldDesc, FieldFlags, GlancesPluginModel};
use super::value::Value;

impl GlancesPluginModel {
    /// Base views rebuild (upstream `update_views` parity): every field
    /// of every element gets a decoration; `log`/`alert` description
    /// flags route through the alert engine, the rest stay DEFAULT.
    /// `descs` is the plugin's `fields_description` table; `key_field`
    /// is its `get_key` element identity (None for scalar plugins).
    pub fn build_views(
        &mut self,
        descs: &[FieldDesc],
        key_field: Option<&str>,
        events: Option<&mut EventLog>,
    ) {
        let mut events_opt = events;
        // Reborrow helper: pass the log along without consuming it, so
        // every field can record its own event.
        fn reborrow<'a, 'b>(
            events: &'b mut Option<&'a mut EventLog>,
        ) -> Option<&'b mut EventLog> {
            events.as_mut().map(|e| &mut **e)
        }
        // Move stats aside instead of cloning: `decorate` needs
        // `&mut self`, so the value can't be borrowed in place.
        let stats = std::mem::replace(&mut self.stats, Value::Null);
        match &stats {
            Value::Array(items) => {
                let mut views = HashMap::new();
                for (i, item) in items.iter().enumerate() {
                    let obj = match item.as_object() {
                        Some(o) => o,
                        None => continue,
                    };
                    let elem = key_field
                        .and_then(|kf| obj.get(kf))
                        .and_then(|v| match v {
                            Value::String(s) if !s.is_empty() => Some(s.clone()),
                            Value::Int(n) => Some(n.to_string()),
                            Value::Uint(n) => Some(n.to_string()),
                            _ => None,
                        })
                        .unwrap_or_else(|| i.to_string());
                    let mut fields = HashMap::new();
                    for (field, v) in obj {
                        let d = self.decorate(descs, field, v.as_f64().unwrap_or(0.0), reborrow(&mut events_opt));
                        fields.insert(field.clone(), d);
                    }
                    views.insert(elem, fields);
                }
                self.views = views;
            }
            Value::Object(map) => {
                let mut fields = HashMap::new();
                for (field, v) in map {
                    let d = self.decorate(descs, field, v.as_f64().unwrap_or(0.0), reborrow(&mut events_opt));
                    fields.insert(field.clone(), d);
                }
                let mut views = HashMap::new();
                views.insert(String::new(), fields);
                self.views = views;
            }
            _ => {
                self.views = HashMap::new();
            }
        }
        self.stats = stats;
    }

    fn decorate(
        &mut self,
        descs: &[FieldDesc],
        field: &str,
        value: f64,
        events: Option<&mut EventLog>,
    ) -> String {
        let flags = descs.iter().find(|d| d.name == field).map(|d| d.flags);
        match flags {
            Some(fl) if fl.contains(FieldFlags::ALERT) => {
                self.get_alert(value, 0.0, 100.0, field, None, false, false, None, events)
            }
            Some(fl) if fl.contains(FieldFlags::LOG) => {
                self.get_alert_log(value, 100.0, field, events)
            }
            _ => "DEFAULT".into(),
        }
    }

}
