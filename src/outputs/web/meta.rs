//! Aggregate metadata endpoints: `/api/all/limits`, `/views`,
//! `/description`, `/api/all/stats`, and `/api/4/history`.
//!
//! All serve live model data (limits maps, view metadata, recorded
//! history) — no placeholders.

use super::health::health_value;
use super::response::Response;
use super::router::{serve_all_values, Ctx};
use crate::core::value::{self, Value};

/// Plugin payloads in the dashboard bundle, in dashboard column
/// order. Heavy sections (processlist, alert log) stay out — the
/// page fetches those on its own slower cadence.
const DASHBOARD_KEYS: &[&str] = &[
    "cpu", "mem", "load", "system", "uptime", "memswap", "processcount",
    "percpu", "network", "connections", "diskio", "fs", "sensors",
    "gpu", "power", "ip",
];

/// `GET /api/4/dashboard` — one round trip for the whole 2s refresh:
/// every fast plugin payload plus the health rollup under `"health"`.
pub(crate) fn serve_dashboard(ctx: &Ctx<'_>) -> Response {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut out = std::collections::BTreeMap::new();
    for key in DASHBOARD_KEYS {
        let v = guard.iter().find(|p| p.name() == *key)
            .map(|p| p.stats().clone())
            .unwrap_or(Value::Null);
        out.insert((*key).to_string(), v);
    }
    out.insert("health".to_string(), health_value(ctx));
    Response::ok_json(value::to_json(&Value::Object(out)))
}

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
                    crate::core::alerts::LimitValue::Float(f) => Value::Float(*f),
                    crate::core::alerts::LimitValue::List(l) => Value::Array(
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
    // View metadata per plugin: element key, declared fields, and live
    // alert decorations (`views` rebuilt every tick by update_views).
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
        if let Some(model) = p.model() {
            let mut deco = std::collections::BTreeMap::new();
            for (elem, fields) in &model.views {
                let mut fm = std::collections::BTreeMap::new();
                for (field, d) in fields {
                    fm.insert(field.clone(), Value::String(d.clone()));
                }
                deco.insert(elem.clone(), Value::Object(fm));
            }
            m.insert("decorations".into(), Value::Object(deco));
        }
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

/// `GET /api/4/status` — health check `{"version": ...}` (upstream
/// `_api_status` parity; container probes use this path).
pub(crate) fn serve_status() -> Response {
    let mut m = std::collections::BTreeMap::new();
    m.insert("version".to_string(), Value::String(env!("CARGO_PKG_VERSION").to_string()));
    Response::ok_json(value::to_json(&Value::Object(m)))
}

/// `GET /api/4/pluginslist` — JSON array of registered plugin names
/// in registration order (upstream `_api_plugins` parity).
pub(crate) fn serve_pluginslist(ctx: &Ctx<'_>) -> Response {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let names: Vec<Value> = guard.iter().map(|p| Value::String(p.name().to_string())).collect();
    Response::ok_json(value::to_json(&Value::Array(names)))
}

/// `GET /api/4/serverslist` — always `[]`: a servers list only exists
/// in client/browser mode (upstream parity for `-w`).
pub(crate) fn serve_serverslist() -> Response { Response::ok_json("[]".into()) }

fn is_ver(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// Last-`nb` slice of a series (`nb=0` = all).
fn history_slice(pts: &[(f64, f64)], nb: usize) -> &[(f64, f64)] {
    if nb == 0 { pts } else { &pts[pts.len().saturating_sub(nb)..] }
}

/// `GET /api/[4/]<plugin>/history[/<nb>]` — that plugin's recorded
/// series as `{field: [[epoch, value], ...]}`, limited to the last
/// `nb` points. Upstream `_api_history` parity, except unknown
/// plugins 404 (house convention; upstream answers 400).
pub(crate) fn serve_plugin_history(path: &str, ctx: &Ctx<'_>) -> Response {
    let rest = match path.strip_prefix("/api/") {
        Some(r) => r,
        None => return Response::not_found(),
    };
    let segs: Vec<&str> = rest.split('/').collect();
    let (name, tail) = match segs.as_slice() {
        [n, "history"] => (*n, None),
        [n, "history", nb] => (*n, Some(*nb)),
        [v, n, "history"] if is_ver(v) => (*n, None),
        [v, n, "history", nb] if is_ver(v) => (*n, Some(*nb)),
        _ => return Response::not_found(),
    };
    let nb: usize = match tail {
        None => 0,
        Some(s) => match s.parse() {
            Ok(n) => n,
            Err(_) => return Response::not_found(),
        },
    };
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let p = match guard.iter().find(|p| p.name() == name) {
        Some(p) => p,
        None => return Response::not_found(),
    };
    let mut out = std::collections::BTreeMap::new();
    if let Some(model) = p.model() {
        for (k, pts) in model.stats_history.snapshot() {
            let arr: Vec<Value> = history_slice(&pts, nb).iter()
                .map(|(t, v)| Value::Array(vec![Value::Float(*t), Value::Float(*v)]))
                .collect();
            out.insert(k, Value::Array(arr));
        }
    }
    Response::ok_json(value::to_json(&Value::Object(out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_slice_takes_last_n() {
        let pts = [(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
        assert_eq!(history_slice(&pts, 0).len(), 3);
        assert_eq!(history_slice(&pts, 2), &[(2.0, 2.0), (3.0, 3.0)]);
        assert_eq!(history_slice(&pts, 9).len(), 3);
        assert!(history_slice(&[], 5).is_empty());
    }
}


