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
fn api_doc_lists_only_served_routes() {
    // Every route the --api-doc-restful dump advertises must reach a
    // handler (status != 404). Catches dump/router drift.
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    for ep in crate::outputs::api_doc::ENDPOINTS {
        if ep.path.contains("{pid}") {
            continue; // pid existence is data, not routing; pinned below.
        }
        let path = ep.path.replace("{plugin}", "cpu").replace("{n}", "1");
        let req = Request { method: ep.method.into(), path,
                            query: String::new(), version: "HTTP/1.1".into(),
                            headers: Default::default(), body: vec![] };
        assert_ne!(route(&req, &ctx).status, 404,
                   "documented route not served: {} {}", ep.method, ep.path);
    }
    // Pid routes: a non-numeric pid reaches the handler (400), which
    // proves the route is served — pid existence itself is data.
    let req = Request { method: "POST".into(),
                        path: "/api/4/processes/extended/abc".into(),
                        query: String::new(), version: "HTTP/1.1".into(),
                        headers: Default::default(), body: vec![] };
    assert_eq!(route(&req, &ctx).status, 400);
    assert_eq!(route(&get("/api/4/processes/extended"), &ctx).status, 200);
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

#[test]
fn root_serves_live_dashboard() {
    // Like upstream, `/` is the live UI — never a landing page.
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    for path in ["/", "/index.html", "/dashboard"] {
        let r = route(&get(path), &ctx);
        assert_eq!(r.status, 200, "{path}");
        let body = String::from_utf8(r.body).unwrap();
        assert!(body.contains("id=\"gpus\""), "{path} must serve the dashboard");
    }
}

#[test]
fn versioned_aggregates_status_and_lists() {
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    for path in ["/api/4/all", "/api/4/all/limits", "/api/4/all/views",
                 "/api/4/status", "/api/4/pluginslist", "/api/4/serverslist"] {
        assert_eq!(route(&get(path), &ctx).status, 200, "{path}");
    }
    let body = String::from_utf8(route(&get("/api/4/status"), &ctx).body).unwrap();
    assert!(body.contains(env!("CARGO_PKG_VERSION")), "status carries version");
    let body = String::from_utf8(route(&get("/api/4/pluginslist"), &ctx).body).unwrap();
    assert!(body.contains("\"cpu\"") && body.contains("\"gpu\""), "pluginslist names plugins");
    let body = String::from_utf8(route(&get("/api/4/serverslist"), &ctx).body).unwrap();
    assert_eq!(body, "[]");
}

#[test]
fn values_and_description_reject_extra_segments() {
    // /api/4/cpu/total/description is an upstream ITEM route, not our
    // whole-plugin description: 404 rather than the wrong shape.
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    assert_eq!(route(&get("/api/4/cpu/total/description"), &ctx).status, 404);
    assert_eq!(route(&get("/api/4/cpu/total/values"), &ctx).status, 404);
    assert_eq!(route(&get("/api/cpu/values"), &ctx).status, 200);
    assert_eq!(route(&get("/api/4/cpu/values"), &ctx).status, 200);
}

#[test]
fn get_process_by_pid_handles_missing_and_extended_wins() {
    // No tick has run, so the processlist cache is empty: any pid
    // 404s. Non-numeric tails 404 too (GET, unlike POST's 400).
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    assert_eq!(route(&get("/api/4/processes/1"), &ctx).status, 404);
    assert_eq!(route(&get("/api/4/processes/nope"), &ctx).status, 404);
    // The extended exact arm still wins over the pid guard.
    assert_eq!(route(&get("/api/4/processes/extended"), &ctx).status, 200);
}

#[test]
fn removed_surfaces_404() {
    // Marketing pages and XML-RPC are gone: the binary serves the
    // dashboard and the REST API only.
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    for path in ["/about", "/about.html", "/browser", "/browser.html"] {
        assert_eq!(route(&get(path), &ctx).status, 404, "{path}");
    }
    let post = Request { method: "POST".into(), path: "/xmlrpc".into(),
        query: String::new(), version: "HTTP/1.1".into(),
        headers: Default::default(), body: vec![] };
    assert_eq!(route(&post, &ctx).status, 404);
}

#[test]
fn plugin_history_routes() {
    let (stats, args) = live_ctx();
    let ctx = test_ctx(&stats, &args);
    // No ticks: empty object, still 200.
    for path in ["/api/4/cpu/history", "/api/4/cpu/history/60", "/api/cpu/history"] {
        assert_eq!(route(&get(path), &ctx).status, 200, "{path}");
    }
    assert_eq!(route(&get("/api/4/nope/history"), &ctx).status, 404);
    assert_eq!(route(&get("/api/4/cpu/history/abc"), &ctx).status, 404);
    // Global history and item paths are untouched.
    assert_eq!(route(&get("/api/4/history"), &ctx).status, 200);
    assert_eq!(route(&get("/api/4/cpu/total/history"), &ctx).status, 404);
}
