//! Shared snapshot flattening for field-iterating exporters.
//!
//! Two plugin stat shapes exist:
//!   - **Object plugins** (`cpu`, `mem`, `load`, …): fields iterate
//!     directly — series `cpu`, field `total`.
//!   - **Array plugins** (`network`, `fs`, `diskio`, `sensors`, …): one
//!     object per element. Each element becomes its own series named
//!     `plugin.<elem>` where `<elem>` is the value of the plugin's
//!     declared `get_key` field (`alias`, `mntpoint`, `disk_name`, …),
//!     falling back to the element index when the key is absent or the
//!     plugin declares no key.
//!
//! Without this, every exporter silently dropped array plugins — network
//! throughput, per-mount fs usage, disk IO and sensor data never left
//! the process.

use std::collections::HashMap;

use crate::core::value::Value;

/// One flattened metric: a `(series, key, value)` triple.
/// `series` is `plugin` or `plugin.<elem>`; `elem` is the raw element
/// identity (for exporters that want it as a tag rather than a name part).
pub struct Field<'a> {
    pub plugin: &'a str,
    pub series: String,
    pub elem: Option<String>,
    pub key: &'a str,
    pub value: &'a Value,
    /// The plugin's element-identity field (`get_key`, e.g.
    /// `interface_name`); `None` for object plugins. Exporters use it
    /// for label/tag names (upstream `keys_name` parity).
    pub key_field: Option<&'a str>,
}

/// Flatten `snap` into field triples. `keys` maps plugin name → the
/// plugin's `get_key` field (from `GlancesStats::plugin_keys()`).
pub fn collect<'a>(snap: &'a Value, keys: &HashMap<String, &'static str>) -> Vec<Field<'a>> {
    let mut out = Vec::new();
    let plugins = match snap.as_object() {
        Some(o) => o,
        None => return out,
    };
    for (plugin, value) in plugins {
        match value {
            Value::Object(fields) => {
                for (k, v) in fields {
                    out.push(Field {
                        plugin, series: plugin.clone(), elem: None,
                        key: k.as_str(), value: v, key_field: None,
                    });
                }
            }
            Value::Array(elems) => {
                let key_field = keys.get(plugin.as_str()).copied();
                for (i, elem) in elems.iter().enumerate() {
                    let obj = match elem.as_object() { Some(o) => o, None => continue };
                    let id = key_field
                        .and_then(|kf| obj.get(kf))
                        .and_then(elem_id_str)
                        .unwrap_or_else(|| i.to_string());
                    for (k, v) in obj {
                        // The identity field itself is not a metric.
                        if Some(k.as_str()) == key_field { continue; }
                        out.push(Field {
                            plugin,
                            series: format!("{}.{}", plugin, id),
                            elem: Some(id.clone()),
                            key: k.as_str(), value: v,
                            key_field,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Render an element's identity value as a series fragment.
fn elem_id_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Int(i) => Some(i.to_string()),
        Value::Uint(u) => Some(u.to_string()),
        _ => None,
    }
}

/// Keep only the listed top-level plugins (`--stdout-csv` /
/// `--stdout-json <list>` parity). `None`/empty spec is a no-op.
pub fn filter_plugins(snapshot: &Value, spec: &Option<String>) -> Value {
    let list: Vec<String> = spec
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if list.is_empty() {
        return snapshot.clone();
    }
    match snapshot {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| list.iter().any(|w| w == *k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn obj(pairs: &[(&str, Value)]) -> Value {
        let mut m = BTreeMap::new();
        for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
        Value::Object(m)
    }

    fn keys() -> HashMap<String, &'static str> {
        let mut k = HashMap::new();
        k.insert("network".to_string(), "interface_name");
        k.insert("fs".to_string(), "mnt_point");
        k
    }

    #[test]
    fn object_plugins_flatten_directly() {
        let snap = obj(&[("cpu", obj(&[("total", Value::Float(3.0))]))]);
        let f = collect(&snap, &keys());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].series, "cpu");
        assert_eq!(f[0].key, "total");
        assert!(f[0].elem.is_none());
    }

    #[test]
    fn array_plugins_use_get_key_element_ids() {
        let nic = obj(&[
            ("interface_name", Value::String("eth0".into())),
            ("rx", Value::Uint(10)),
        ]);
        let snap = obj(&[("network", Value::Array(vec![nic]))]);
        let f = collect(&snap, &keys());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].series, "network.eth0");
        assert_eq!(f[0].key, "rx");
        assert_eq!(f[0].elem.as_deref(), Some("eth0"));
    }

    #[test]
    fn array_elements_fall_back_to_index_without_key() {
        let mnt = obj(&[
            ("mnt_point", Value::String("/".into())),
            ("used", Value::Uint(1)),
        ]);
        let no_key = obj(&[("used", Value::Uint(2))]);
        let snap = obj(&[("fs", Value::Array(vec![mnt, no_key]))]);
        let mut keys = HashMap::new(); // empty map → all index fallback
        let f = collect(&snap, &keys);
        // No key field is declared, so `mntpoint` is emitted as an
        // ordinary metric — 3 fields total: mntpoint, used (elem 0),
        // used (elem 1).
        assert_eq!(f[0].series, "fs.0");
        assert_eq!(f[1].series, "fs.0");
        assert_eq!(f[2].series, "fs.1");
        keys.insert("fs".to_string(), "mnt_point");
        let f = collect(&snap, &keys);
        assert_eq!(f[0].series, "fs./");
        assert_eq!(f[1].series, "fs.1");
    }

    #[test]
    fn key_field_is_not_emitted_as_a_metric() {
        let nic = obj(&[
            ("interface_name", Value::String("lo".into())),
            ("tx", Value::Uint(5)),
        ]);
        let snap = obj(&[("network", Value::Array(vec![nic]))]);
        let f = collect(&snap, &keys());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].key, "tx");
    }
}
