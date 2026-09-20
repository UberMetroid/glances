//! Aggregate metadata endpoints: `/api/all/limits`, `/views`,
//! `/description`, `/api/all/stats`, and `/api/4/history`.
//!
//! All serve live model data (limits maps, view metadata, recorded
//! history) — no placeholders.

use super::response::Response;
use super::router::{serve_all_values, Ctx};
use crate::core::value::{self, Value};

pub(crate) fn serve_all_limits(ctx: &Ctx<'_>) -> Response {
    // Real limits export: each plugin model carries the parsed
    // `[<plugin>] careful/warning/critical` config map (empty by default).
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut out = std::collections::BTreeMap::new();
    for p in guard.iter() {
        let mut m = std::collections::BTreeMap::new();
        if let Some(model) = p.model() {
            for (k, v) in &model.limits {
                let val = match v {
                    crate::core::plugin::LimitValue::Float(f) => Value::Float(*f),
                    crate::core::plugin::LimitValue::List(l) => Value::Array(
                        l.iter().map(|s| Value::String(s.clone())).collect(),
                    ),
                };
                m.insert(k.clone(), val);
            }
        }
        out.insert(p.name().to_string(), Value::Object(m));
    }
    Response::ok_json(value::to_json(&Value::Object(out)))
}

pub(crate) fn serve_all_views(ctx: &Ctx<'_>) -> Response {
    // View metadata: element key + declared fields per plugin (what the
    // WebUI/TUI uses to label columns; empty fields = free-form stats).
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut out = std::collections::BTreeMap::new();
    for p in guard.iter() {
        let mut m = std::collections::BTreeMap::new();
        match p.get_key() {
            Some(k) => {
                m.insert("key".into(), Value::String(k.to_string()));
            }
            None => {
                m.insert("key".into(), Value::Null);
            }
        }
        m.insert(
            "fields".into(),
            Value::Array(
                p.fields_description()
                    .iter()
                    .map(|f| Value::String(f.name.to_string()))
                    .collect(),
            ),
        );
        out.insert(p.name().to_string(), Value::Object(m));
    }
    Response::ok_json(value::to_json(&Value::Object(out)))
}
pub(crate) fn serve_all_description(ctx: &Ctx<'_>) -> Response { serve_all_views(ctx) }
pub(crate) fn serve_all_stats(ctx: &Ctx<'_>) -> Response { serve_all_values(ctx) }

pub(crate) fn serve_history(ctx: &Ctx<'_>) -> Response {
    // Recorded per-plugin numeric history: {plugin: {key: [[ts, v], …]}}.
    // Empty until refresh ticks record (or when --disable-history is set).
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut out = std::collections::BTreeMap::new();
    for p in guard.iter() {
        let mut m = std::collections::BTreeMap::new();
        if let Some(model) = p.model() {
            for (k, pts) in model.stats_history.snapshot() {
                m.insert(
                    k,
                    Value::Array(
                        pts.iter()
                            .map(|(t, v)| {
                                Value::Array(vec![Value::Float(*t), Value::Float(*v)])
                            })
                            .collect(),
                    ),
                );
            }
        }
        out.insert(p.name().to_string(), Value::Object(m));
    }
    Response::ok_json(value::to_json(&Value::Object(out)))
}


