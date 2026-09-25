//! Dispatch: each (method, path) to its handler, 404 otherwise.
//!
//! The auth gate runs before everything. Arm order is load-bearing:
//! specific paths (history, event stream, extended process) precede
//! the generic direct-plugin arm, which 404s multi-segment leftovers
//! rather than serve a wrong shape with 200.

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

/// Handler context: shared stats, args, credentials, and the refresh
/// sequence counter.
pub struct Ctx<'a> {
    pub stats: &'a GlancesStats,
    pub args: &'a Args,
    pub password: &'a PasswordFile,
    pub auth_enabled: bool,
    pub api_key: Option<String>,
    pub refresh_seq: Arc<std::sync::atomic::AtomicU64>,
}

/// Route one request to its response.
pub fn route(req: &Request, ctx: &Ctx<'_>) -> Response {
    if auth::gate_applies(req.path.as_str(), ctx.auth_enabled, ctx.api_key.is_some())
        && !auth::credentials_ok(req, ctx.password, ctx.auth_enabled, ctx.api_key.as_deref()) {
            return if ctx.auth_enabled { Response::unauthorized() } else { Response::unauthorized_key() };
        }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") | ("GET", "/dashboard") => serve_static("dashboard.html"),
        ("GET", "/favicon.ico") => serve_static("favicon.ico"),
        ("GET", "/openapi.json") => serve_static("openapi.json"),
        ("GET", "/api/all/values") | ("GET", "/api/4/all") => serve_all_values(ctx),
        ("GET", "/api/all/limits") | ("GET", "/api/4/all/limits") => meta::serve_all_limits(ctx),
        ("GET", "/api/all/views") | ("GET", "/api/4/all/views") => meta::serve_all_views(ctx),
        ("GET", "/api/4/status") => meta::serve_status(),
        ("GET", "/api/4/pluginslist") => meta::serve_pluginslist(ctx),
        ("GET", "/api/4/serverslist") => meta::serve_serverslist(),
        ("GET", "/api/4/health") | ("GET", "/api/health") => health::serve_health(ctx),
        ("GET", "/api/4/dashboard") => meta::serve_dashboard(ctx),
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
    let Some((ct, bytes)) = static_fs::lookup(name) else {
        return Response::not_found();
    };
    // Bundled pages never change within a release: 5 minutes of
    // browser caching skips most reload bytes.
    Response::ok_bytes(bytes.to_vec(), ct).header("Cache-Control", "public, max-age=300")
}

/// Name → stats across every registered plugin.
fn snapshot_plugins(ctx: &Ctx<'_>) -> Value {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    Value::Object(guard.iter().map(|p| (p.name().to_string(), p.stats().clone())).collect())
}

pub(crate) fn serve_all_values(ctx: &Ctx<'_>) -> Response {
    Response::ok_json(value::to_json(&snapshot_plugins(ctx)))
}

fn serve_plugin_values(path: &str, ctx: &Ctx<'_>) -> Response {
    // `/api/<name>/values` (a longer same-suffix form carries the same
    // payload through the same extractor).
    let name = extract_plugin_name(path, "/values").unwrap_or_default();
    lookup_plugin(&name, ctx).map_or_else(Response::not_found, |stats| {
        Response::ok_json(value::to_json(&stats))
    })
}

/// Pull the plugin name from a suffixed path: `/api/<name><suffix>`
/// or `/api/<version>/<name><suffix>` (a purely numeric first segment
/// is a version). Empty names and trailing segments refuse.
fn extract_plugin_name(path: &str, suffix: &str) -> Option<String> {
    let rest = path.strip_suffix(suffix)?.trim_end_matches('/').strip_prefix("/api/")?;
    let mut segs = rest.split('/');
    let first = segs.next()?;
    // A purely numeric first segment is a version ("4"), never a name
    // (note: the empty string counts as numeric — it then fails below).
    let name = if first.chars().all(|c| c.is_ascii_digit()) { segs.next()? } else { first };
    if name.is_empty() || segs.next().is_some() {
        None
    } else {
        Some(name.to_string())
    }
}

fn lookup_plugin(name: &str, ctx: &Ctx<'_>) -> Option<Value> {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    guard.iter().find(|p| p.name() == name).map(|p| p.stats().clone())
}

fn serve_plugin_by_name(name: &'static str, ctx: &Ctx<'_>) -> Response {
    lookup_plugin(name, ctx).map_or_else(Response::not_found, |stats| {
        Response::ok_json(value::to_json(&stats))
    })
}

/// Direct payloads `/api/<name>` and `/api/<version>/<name>` (what the
/// homepage widget polls). Anything multi-segment 404s.
fn serve_plugin_direct(path: &str, ctx: &Ctx<'_>) -> Response {
    let Some(rest) = path.strip_prefix("/api/") else {
        return Response::not_found();
    };
    let mut segs = rest.split('/');
    let first = segs.next().unwrap_or("");
    let numeric = !first.is_empty() && first.chars().all(|c| c.is_ascii_digit());
    let name = if numeric { segs.next().unwrap_or("") } else { first };
    if name.is_empty() || segs.next().is_some() {
        return Response::not_found();
    }
    lookup_plugin(name, ctx).map_or_else(Response::not_found, |stats| {
        Response::ok_json(value::to_json(&stats))
    })
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
                Value::Object(
                    [
                        ("name".to_string(), Value::String(f.name.to_string())),
                        ("unit".to_string(), Value::String(format!("{:?}", f.unit))),
                    ]
                    .into_iter()
                    .collect(),
                )
            }).collect();
            Response::ok_json(value::to_json(&Value::Array(fields)))
        }
        None => Response::not_found(),
    }
}

/// One-shot event frame (proves the SSE framing; the connection then
/// closes — the long-lived stream is a later milestone).
fn serve_sse(_ctx: &Ctx<'_>) -> Response {
    let frame = sse::format_event("hello", r#"{"msg":"glances-rs"}"#, Some(1));
    let mut headers = sse::response_headers();
    headers.push(("Content-Length", frame.len().to_string()));
    let mut r = Response::ok_bytes(frame.into_bytes(), "text/event-stream");
    r.headers.clear();
    for (k, v) in headers {
        r = r.header(k, v);
    }
    r
}

/// Test context without real credentials (gate open).
#[cfg(test)]
pub fn test_ctx<'a>(stats: &'a GlancesStats, args: &'a Args) -> Ctx<'a> {
    static EMPTY_PW: std::sync::OnceLock<PasswordFile> = std::sync::OnceLock::new();
    let pw = EMPTY_PW.get_or_init(PasswordFile::empty);
    Ctx { stats, args, password: pw, auth_enabled: false, api_key: None,
          refresh_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)) }
}

/// MCP endpoint: dispatch the JSON-RPC body, answer JSON.
fn serve_mcp(req: &Request, ctx: &Ctx<'_>) -> Response {
    let body = std::str::from_utf8(&req.body).unwrap_or("");
    Response::ok_bytes(crate::outputs::mcp::handle(body, ctx.stats).into_bytes(), "application/json")
}
