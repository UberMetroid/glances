//! Snapshot plugin filtering for `--stdout-csv` / `--stdout-json`.
//!
//! Keeps only the listed top-level plugins (`--stdout-csv <list>`
//! parity). `None`/empty spec is a no-op. Pure pull-side shaping:
//! nothing here sends data anywhere.

use crate::core::value::Value;

/// Keep only the listed top-level plugins.
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
