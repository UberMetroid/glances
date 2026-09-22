//! Tests for the live stats dashboard route (`/dashboard`).

use crate::cli::args::{Args, Mode};
use crate::core::stats::GlancesStats;
use crate::outputs::web::request::Request;
use crate::outputs::web::router::{route, test_ctx};
use crate::plugins;

#[test]
fn dashboard_route_serves_html() {
    let stats = GlancesStats::new(2.0);
    plugins::register_all(&stats);
    let args = Args { mode: Mode::WebServer, ..Args::default() };
    let ctx = test_ctx(&stats, &args);
    let req = Request { method: "GET".into(), path: "/dashboard".into(),
                        query: String::new(), version: "HTTP/1.1".into(),
                        headers: Default::default(), body: vec![] };
    let r = route(&req, &ctx);
    assert_eq!(r.status, 200);
    assert_eq!(r.headers.get("Content-Type").map(String::as_str),
               Some("text/html; charset=utf-8"));
    let body = String::from_utf8(r.body).unwrap();
    assert!(body.contains("SLOW_KEYS"), "dashboard must tier heavy polls");
    assert!(body.contains("api/4/dashboard"), "dashboard must fetch one bundle per fast tick");
    assert!(body.contains("id=\"gpus\""), "dashboard must render the GPU section");
    assert!(body.contains("id=\"percpu\""), "dashboard must render per-core CPU");
    assert!(body.contains("gphead"), "dashboard must group GPUs internal/external");
    assert!(body.contains("MEM "), "dashboard must render GPU memory");
    assert!(body.contains("transcoding"), "dashboard must render GPU transcode state");
    assert!(body.contains("chip-jellyfin"), "dashboard must render service chips");
    assert!(body.contains("data-key"), "process headers must be sortable");
    assert!(body.contains("table-layout: fixed"),
            "process table must use fixed layout so headers never shift on refresh");
    assert!(body.contains("<colgroup>"),
            "process table must pin column widths via colgroup");
    assert_eq!(body.matches("<tr class=\"sk\">").count(), 30,
               "dashboard must ship a 30-row process skeleton for shift-free first paint");
    assert!(body.contains("mini sk"), "dashboard must skeleton per-core/fs rows");
    assert!(body.contains("gphead sk"), "dashboard must skeleton GPU groups");
    assert!(body.contains("d.processlist !== undefined"),
            "dashboard must keep skeleton until real process data arrives");
    assert!(body.contains(".sk td"), "dashboard must dim skeleton rows");
    assert!(body.contains("id=\"ticker\""), "dashboard must render the health ticker");
    assert!(body.contains("d.health"), "dashboard must read the health rollup from the bundle");
    assert!(body.contains("id=\"banner\""), "dashboard must render the alert banner");
    assert!(body.contains("id=\"spark-power\""), "dashboard must render the power section");
    assert!(body.contains("id=\"conns\""), "dashboard must render connections");
    assert!(body.contains("id=\"spark-net\""), "dashboard must render the network sparkline");
    assert!(body.contains("procFilter"), "dashboard must filter processes as you type");
    assert!(body.contains("tickscroll"), "ticker must animate");
    assert!(body.contains("X-API-Key"), "dashboard must send the API key header");
    assert!(body.contains("glances_key"), "dashboard must prompt for and store the API key");
}

#[test]
fn dashboard_bundle_serves_fast_keys_and_health() {
    let stats = GlancesStats::new(2.0);
    plugins::register_all(&stats);
    let args = Args { mode: Mode::WebServer, ..Args::default() };
    let ctx = test_ctx(&stats, &args);
    let req = Request { method: "GET".into(), path: "/api/4/dashboard".into(),
                        query: String::new(), version: "HTTP/1.1".into(),
                        headers: Default::default(), body: vec![] };
    let r = route(&req, &ctx);
    assert_eq!(r.status, 200);
    let body = String::from_utf8(r.body).unwrap();
    for key in ["\"cpu\"", "\"processcount\"", "\"power\"", "\"connections\"", "\"health\""] {
        assert!(body.contains(key), "bundle must embed {key}");
    }
    assert!(!body.contains("\"processlist\""), "bundle must stay slim (no processlist)");
}
