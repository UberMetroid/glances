//! Request → Response dispatch.
//!
//! The router is a flat `match` over `(method, path)`. Anything we don't
//! recognize returns 404. We intentionally don't auto-handle OPTIONS /
//! HEAD — the Python Glances web UI doesn't need them either.
//!
//! Per plan §6.2 AC-16, the REST surface ships in a follow-up milestone;
//! this file lays the wiring and serves the most-requested endpoints
//! (`/api/all/values`, `/api/all/limits`, `/api/<plugin>/description`)
//! plus the SPA root + favicon + a single SSE stream.

use std::sync::Arc;

use super::auth;
use super::health;
use super::meta;
use super::mutate;
use super::request::Request;
use super::response::Response;
use super::sse;
use super::static_fs;
use crate::cli::args::Args;
use crate::core::password::PasswordFile;
use crate::core::stats::GlancesStats;
use crate::core::value::{self, Value};

/// Shared context handed to every handler. Holds only `&'static`-style
/// references (the underlying `GlancesStats` is the long-lived piece).
pub struct Ctx<'a> {
    pub stats: &'a GlancesStats,
    pub args: &'a Args,
    pub password: &'a PasswordFile,
    pub auth_enabled: bool,
    pub api_key: Option<String>,
    pub refresh_seq: Arc<std::sync::atomic::AtomicU64>,
}

/// Top-level dispatch entry point. Returns the response to write.
pub fn route(req: &Request, ctx: &Ctx<'_>) -> Response {
    // Auth gate: Basic and/or API key (see auth::gate_applies for
    // the favicon + dashboard-shell exemptions).
    if auth::gate_applies(req.path.as_str(), ctx.auth_enabled, ctx.api_key.is_some()) {
        if !auth::credentials_ok(req, ctx.password, ctx.auth_enabled, ctx.api_key.as_deref()) {
            return if ctx.auth_enabled { Response::unauthorized() } else { Response::unauthorized_key() };
        }
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") | ("GET", "/dashboard") => serve_static("dashboard.html"),
        ("GET", "/favicon.ico") => serve_static("favicon.ico"),
        ("GET", "/openapi.json") => serve_static("openapi.json"),
        ("GET", "/api/all/values") => serve_all_values(ctx),
        ("GET", "/api/all/limits") => meta::serve_all_limits(ctx),
        ("GET", "/api/all/views") => meta::serve_all_views(ctx),
        ("GET", "/api/4/all") => serve_all_values(ctx),
        ("GET", "/api/4/all/limits") => meta::serve_all_limits(ctx),
        ("GET", "/api/4/all/views") => meta::serve_all_views(ctx),
        ("GET", "/api/4/status") => meta::serve_status(),
        ("GET", "/api/4/pluginslist") => meta::serve_pluginslist(ctx),
        ("GET", "/api/4/serverslist") => meta::serve_serverslist(),
        ("GET", "/api/4/health") | ("GET", "/api/health") => health::serve_health(ctx),
        ("GET", "/api/all/description") => meta::serve_all_description(ctx),
        ("GET", "/api/all/stats") => meta::serve_all_stats(ctx),
        ("GET", path) if path.starts_with("/api/") && path.ends_with("/values") => {
            serve_plugin_values(path, ctx)
        }
        ("GET", path) if path.starts_with("/api/") && path.ends_with("/description") => {
            serve_plugin_description(path, ctx)
        }
        ("GET", "/api/4/history") => meta::serve_history(ctx),
        ("GET", "/api/4/cpu") | ("GET", "/api/cpu") => serve_plugin_by_name("cpu", ctx),
        ("GET", "/api/4/mem") | ("GET", "/api/mem") => serve_plugin_by_name("mem", ctx),
        ("GET", "/api/4/load") | ("GET", "/api/load") => serve_plugin_by_name("load", ctx),
        ("GET", path) if path.starts_with("/api/4/events/stream") => serve_sse(ctx),
        ("GET", "/healthz") => Response::ok_text("ok\n".into()),
        // POST mutators (upstream `_router` POST block parity).
        ("POST", "/api/4/events/clear/warning") | ("POST", "/api/events/clear/warning") =>
            mutate::clear_events(ctx, false),
        ("POST", "/api/4/events/clear/all") | ("POST", "/api/events/clear/all") =>
            mutate::clear_events(ctx, true),
        ("POST", "/api/4/processes/extended/disable")
        | ("POST", "/api/processes/extended/disable") =>
            mutate::disable_extended(ctx),
        ("GET", "/api/4/processes/extended") | ("GET", "/api/processes/extended") =>
            mutate::serve_extended_process(ctx),
        ("GET", path) if path.starts_with("/api/4/processes/") => mutate::serve_process_by_pid(path, ctx),
        ("GET", path) if path.contains("/history") => meta::serve_plugin_history(path, ctx),
        // Generic direct-plugin arm LAST among the GETs: anything more
        // specific above (history, events stream, extended) wins.
        ("GET", path) if path.starts_with("/api/") => serve_plugin_direct(path, ctx),
        ("POST", path) if path.starts_with("/api/") && path.contains("/processes/extended/") =>
            mutate::serve_set_extended_process(path, ctx),
        ("POST", "/api/4/token") | ("POST", "/api/token") => Response::not_implemented(
            "JWT authentication is not available in this build (pure-std, no token issuer).",
        ),
        ("POST", path) if path == ctx.args.mcp_path.as_str()
            || path == "/mcp" || path == "/mcp/" => serve_mcp(req, ctx),
        _ => Response::not_found(),
    }
}

fn serve_static(name: &'static str) -> Response {
    let (ct, bytes) = match static_fs::lookup(name) {
        Some(t) => t,
        None => return Response::not_found(),
    };
    Response::ok_bytes(bytes.to_vec(), ct)
}

fn snapshot_plugins(ctx: &Ctx<'_>) -> Value {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mut map = std::collections::BTreeMap::new();
    for p in guard.iter() {
        map.insert(p.name().to_string(), p.stats().clone());
    }
    Value::Object(map)
}

pub(crate) fn serve_all_values(ctx: &Ctx<'_>) -> Response { Response::ok_json(value::to_json(&snapshot_plugins(ctx))) }

fn serve_plugin_values(path: &str, ctx: &Ctx<'_>) -> Response {
    // /api/<name>/values or /api/<view>/<name>/values — we only handle the
    // short form here (the longer form is the same payload).
    let name = extract_plugin_name(path, "/values").unwrap_or_else(|| "".to_string());
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let p = match guard.iter().find(|p| p.name() == name) {
        Some(p) => p,
        None => return Response::not_found(),
    };
    Response::ok_json(value::to_json(p.stats()))
}

fn extract_plugin_name(path: &str, suffix: &str) -> Option<String> {
    let rest = path.strip_suffix(suffix)?.trim_end_matches('/');
    let rest = rest.strip_prefix("/api/")?;
    let mut segs = rest.split('/');
    let seg = segs.next()?;
    // `/api/<name>/values` or `/api/<version>/<name>/values` — a purely
    // numeric first segment is an API version, not a plugin name.
    let name = if seg.chars().all(|c| c.is_ascii_digit()) {
        segs.next()?
    } else {
        seg
    };
    // Extra segments are upstream ITEM routes (/cpu/total/...), not
    // our whole-plugin payloads: never serve the wrong shape with 200.
    if name.is_empty() || segs.next().is_some() { None } else { Some(name.to_string()) }
}

fn serve_plugin_by_name(name: &'static str, ctx: &Ctx<'_>) -> Response {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    match guard.iter().find(|p| p.name() == name) {
        Some(p) => Response::ok_json(value::to_json(p.stats())),
        None => Response::not_found(),
    }
}

/// Direct plugin payloads: `/api/<name>` and `/api/<version>/<name>`
/// (upstream REST parity — homepage's glances widget polls `/api/4/cpu`,
/// `/api/4/gpu`, ...). Multi-segment paths the specific arms above
/// didn't claim 404.
fn serve_plugin_direct(path: &str, ctx: &Ctx<'_>) -> Response {
    let rest = match path.strip_prefix("/api/") {
        Some(r) => r,
        None => return Response::not_found(),
    };
    let mut segs = rest.split('/');
    let first = segs.next().unwrap_or("");
    let name = if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) {
        segs.next().unwrap_or("")
    } else {
        first
    };
    if name.is_empty() || segs.next().is_some() {
        return Response::not_found();
    }
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    match guard.iter().find(|p| p.name() == name) {
        Some(p) => Response::ok_json(value::to_json(p.stats())),
        None => Response::not_found(),
    }
}

fn serve_plugin_description(path: &str, ctx: &Ctx<'_>) -> Response {
    let name = match extract_plugin_name(path, "/description") {
        Some(n) => n,
        None => return Response::not_found(),
    };
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    match guard.iter().find(|p| p.name() == name) {
        Some(p) => {
            let fields: Vec<Value> = p.fields_description().iter().map(|f| {
                let mut m = std::collections::BTreeMap::new();
                m.insert("name".into(), Value::String(f.name.to_string()));
                m.insert("unit".into(), Value::String(format!("{:?}", f.unit)));
                Value::Object(m)
            }).collect();
            Response::ok_json(value::to_json(&Value::Array(fields)))
        }
        None => Response::not_found(),
    }
}

/// One-shot SSE response — single event, connection closes. The full
/// long-lived stream lands in a follow-up; this proves the framing works.
fn serve_sse(_ctx: &Ctx<'_>) -> Response {
    let frame = sse::format_event("hello", r#"{"msg":"glances-rs"}"#, Some(1));
    let mut headers = sse::response_headers();
    headers.push(("Content-Length", frame.len().to_string()));
    let mut r = Response::ok_bytes(frame.into_bytes(), "text/event-stream");
    r.headers.clear();
    for (k, v) in headers { r = r.header(k, v); }
    r
}

/// Convenience for tests: build a Ctx without a real password file.
#[cfg(test)]
pub fn test_ctx<'a>(stats: &'a GlancesStats, args: &'a Args) -> Ctx<'a> {
    static EMPTY_PW: std::sync::OnceLock<PasswordFile> = std::sync::OnceLock::new();
    let pw = EMPTY_PW.get_or_init(PasswordFile::empty);
    Ctx { stats, args, password: pw, auth_enabled: false, api_key: None,
          refresh_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)) }
}

/// MCP handler: read the JSON-RPC body, dispatch via `mcp::handle`.
fn serve_mcp(req: &Request, ctx: &Ctx<'_>) -> Response {
    let body = std::str::from_utf8(&req.body).unwrap_or("");
    let response = crate::outputs::mcp::handle(body, ctx.stats);
    Response::ok_bytes(response.into_bytes(), "application/json")
}

