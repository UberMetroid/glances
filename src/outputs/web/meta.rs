//! Aggregate endpoints: the dashboard bundle, limits/views/history
//! exports, and service metadata. Everything serves live model data.

use super::health::health_value;
use super::response::Response;
use super::router::{serve_all_values, Ctx};
use crate::core::value::{self, Value};

/// Dashboard bundle payloads in column order. Heavy sections stay
/// out — the page fetches those on its own slower cadence.
const DASHBOARD_KEYS: &[&str] = &[
    "cpu", "mem", "load", "system", "uptime", "memswap", "processcount",
    "percpu", "network", "connections", "diskio", "fs", "sensors",
    "gpu", "power", "pressure", "ip",
];

/// `GET /api/4/dashboard` — one round trip per refresh: every fast
/// payload plus the health rollup under `"health"`.
pub(crate) fn serve_dashboard(ctx: &Ctx<'_>) -> Response {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut out: std::collections::BTreeMap<String, Value> = DASHBOARD_KEYS
        .iter()
        .map(|k| {
            let v = guard
                .iter()
                .find(|p| p.name() == *k)
                .map(|p| p.stats().clone())
                .unwrap_or(Value::Null);
            ((*k).to_string(), v)
        })
        .collect();
    out.insert("health".to_string(), health_value(ctx));
    Response::ok_json(value::to_json(&Value::Object(out)))
}

/// `GET /api/4/all/limits` — every plugin's configured limits table.
pub(crate) fn serve_all_limits(ctx: &Ctx<'_>) -> Response {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut out = std::collections::BTreeMap::new();
    for p in guard.iter() {
        let mut m = std::collections::BTreeMap::new();
        if let Some(model) = p.model() {
            for (k, v) in &model.limits {
                m.insert(k.clone(), limit_value(v));
            }
        }
        out.insert(p.name().to_string(), Value::Object(m));
    }
    Response::ok_json(value::to_json(&Value::Object(out)))
}

fn limit_value(v: &crate::core::alerts::LimitValue) -> Value {
    match v {
        crate::core::alerts::LimitValue::Float(f) => Value::Float(*f),
        crate::core::alerts::LimitValue::List(l) => {
            Value::Array(l.iter().map(|s| Value::String(s.clone())).collect())
        }
    }
}

/// `GET /api/4/all/views` — per plugin: element key, declared field
/// names, and the live alert decorations.
pub(crate) fn serve_all_views(ctx: &Ctx<'_>) -> Response {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut out = std::collections::BTreeMap::new();
    for p in guard.iter() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            "key".into(),
            p.get_key().map(|k| Value::String(k.to_string())).unwrap_or(Value::Null),
        );
        m.insert(
            "fields".into(),
            Value::Array(p.fields_description().iter().map(|f| Value::String(f.name.to_string())).collect()),
        );
        if let Some(model) = p.model() {
            let mut deco = std::collections::BTreeMap::new();
            for (elem, fields) in &model.views {
                let fm: std::collections::BTreeMap<String, Value> = fields
                    .iter()
                    .map(|(f, d)| (f.clone(), Value::String(d.clone())))
                    .collect();
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

/// `GET /api/4/history` — recorded series per plugin as
/// `{plugin: {series: [[epoch, value], …]}}`. Empty until ticks
/// record (or always, with history disabled).
pub(crate) fn serve_history(ctx: &Ctx<'_>) -> Response {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut out = std::collections::BTreeMap::new();
    for p in guard.iter() {
        let mut m = std::collections::BTreeMap::new();
        if let Some(model) = p.model() {
            for (k, pts) in model.stats_history.snapshot() {
                m.insert(k, points_array(&pts));
            }
        }
        out.insert(p.name().to_string(), Value::Object(m));
    }
    Response::ok_json(value::to_json(&Value::Object(out)))
}

fn points_array(pts: &[(f64, f64)]) -> Value {
    Value::Array(pts.iter().map(|(t, v)| Value::Array(vec![Value::Float(*t), Value::Float(*v)])).collect())
}

/// `GET /api/4/status` — liveness probe answering `{"version": …}`.
pub(crate) fn serve_status() -> Response {
    let m = std::collections::BTreeMap::from([(
        "version".to_string(),
        Value::String(env!("CARGO_PKG_VERSION").to_string()),
    )]);
    Response::ok_json(value::to_json(&Value::Object(m)))
}

/// `GET /api/4/pluginslist` — registered names in order.
pub(crate) fn serve_pluginslist(ctx: &Ctx<'_>) -> Response {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let names: Vec<Value> = guard.iter().map(|p| Value::String(p.name().to_string())).collect();
    Response::ok_json(value::to_json(&Value::Array(names)))
}

/// `GET /api/4/serverslist` — always `[]` (a servers list only exists
/// in client mode).
pub(crate) fn serve_serverslist() -> Response { Response::ok_json("[]".into()) }

fn is_ver(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// Last-`nb` points of a series (`nb=0` keeps everything).
fn history_slice(pts: &[(f64, f64)], nb: usize) -> &[(f64, f64)] {
    if nb == 0 { pts } else { &pts[pts.len().saturating_sub(nb)..] }
}

/// `GET /api/[4/]<plugin>/history[/<nb>]` — one plugin's series as
/// `{field: [[epoch, value], …]}`. Bad shapes, bad counts, and unknown
/// plugins all 404.
pub(crate) fn serve_plugin_history(path: &str, ctx: &Ctx<'_>) -> Response {
    let Some(rest) = path.strip_prefix("/api/") else {
        return Response::not_found();
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
    let Some(p) = guard.iter().find(|p| p.name() == name) else {
        return Response::not_found();
    };
    let mut out = std::collections::BTreeMap::new();
    if let Some(model) = p.model() {
        for (k, pts) in model.stats_history.snapshot() {
            out.insert(k, points_array(history_slice(&pts, nb)));
        }
    }
    Response::ok_json(value::to_json(&Value::Object(out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tail_slicing_rules() {
        let pts = [(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
        assert_eq!(history_slice(&pts, 0).len(), 3);
        assert_eq!(history_slice(&pts, 2), &[(2.0, 2.0), (3.0, 3.0)]);
        assert_eq!(history_slice(&pts, 9).len(), 3);
        assert!(history_slice(&[], 5).is_empty());
    }
}
