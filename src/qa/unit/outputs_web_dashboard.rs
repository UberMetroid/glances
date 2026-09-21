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
    assert!(body.contains("api/all/values"), "dashboard must poll the live API");
    assert!(body.contains("id=\"gpus\""), "dashboard must render the GPU section");
    assert!(body.contains("id=\"percpu\""), "dashboard must render per-core CPU");
    assert!(body.contains("gphead"), "dashboard must group GPUs internal/external");
    assert!(body.contains("MEM "), "dashboard must render GPU memory");
    assert!(body.contains("data-key"), "process headers must be sortable");
    assert_eq!(body.matches("<tr class=\"sk\">").count(), 30,
               "dashboard must ship a 30-row process skeleton for shift-free first paint");
    assert!(body.contains("mini sk"), "dashboard must skeleton per-core/fs rows");
    assert!(body.contains("gphead sk"), "dashboard must skeleton GPU groups");
    assert!(body.contains("d.processlist !== undefined"),
            "dashboard must keep skeleton until real process data arrives");
    assert!(body.contains(".sk td"), "dashboard must dim skeleton rows");
}
