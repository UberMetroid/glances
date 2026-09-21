//! Tests for the web router: direct plugin routes (`/api/<plugin>`,
//! `/api/4/<plugin>`) — the shape homepage's glances widget polls.

use crate::cli::args::{Args, Mode};
use crate::core::stats::GlancesStats;
use crate::outputs::web::request::Request;
use crate::outputs::web::router::{route, test_ctx};
use crate::plugins;

fn get(path: &str) -> Request {
    Request { method: "GET".into(), path: path.into(),
              query: String::new(), version: "HTTP/1.1".into(),
              headers: Default::default(), body: vec![] }
}

fn live_ctx() -> (GlancesStats, Args) {
    let stats = GlancesStats::new(2.0);
    plugins::register_all(&stats);
    (stats, Args { mode: Mode::WebServer, ..Args::default() })
}

#[test]
fn health_endpoint_returns_ok() {
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    assert_eq!(route(&get("/healthz"), &ctx).status, 200);
}

#[test]
fn unknown_path_is_404() {
    let stats = GlancesStats::new(2.0);
    let args = Args { mode: Mode::WebServer, ..Args::default() };
    let ctx = test_ctx(&stats, &args);
    assert_eq!(route(&get("/nope"), &ctx).status, 404);
}

#[test]
fn versioned_plugin_values_route() {
    // Regression: /api/4/<plugin>/values looked for a plugin named
    // "4". The numeric first segment is an API version.
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    assert_eq!(route(&get("/api/cpu/values"), &ctx).status, 200);
    assert_eq!(route(&get("/api/4/cpu/values"), &ctx).status, 200);
    assert_eq!(route(&get("/api/4/nonexistent/values"), &ctx).status, 404);
}

#[test]
fn direct_plugin_routes_serve_widget_endpoints() {
    // Homepage's glances widget polls /api/4/<plugin> directly.
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    for p in ["cpu", "mem", "quicklook", "gpu", "fs", "network"] {
        assert_eq!(route(&get(&format!("/api/4/{p}")), &ctx).status, 200, "{p}");
        assert_eq!(route(&get(&format!("/api/{p}")), &ctx).status, 200, "{p}");
    }
}

#[test]
fn direct_plugin_unknown_names_404() {
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    for path in ["/api/4/nope", "/api/nope", "/api/4/", "/api/"] {
        assert_eq!(route(&get(path), &ctx).status, 404, "{path}");
    }
}

#[test]
fn direct_plugin_rejects_multi_segment_paths() {
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    // Not a direct payload: falls through the generic arm to 404.
    assert_eq!(route(&get("/api/4/cpu/extra"), &ctx).status, 404);
    // POST-only mutator stays 404 on GET.
    assert_eq!(route(&get("/api/4/events/clear/all"), &ctx).status, 404);
}

#[test]
fn versioned_history_not_shadowed_by_generic_arm() {
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    assert_eq!(route(&get("/api/4/history"), &ctx).status, 200);
}
