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
    assert!(body.contains("id=\"warnings\""), "dashboard must render the warnings section");
    assert!(body.contains("id=\"spark-power\""), "dashboard must render the power section");
    assert!(body.contains("id=\"conns\""), "dashboard must render connections");
    assert!(body.contains("id=\"spark-net\""), "dashboard must render the network sparkline");
    assert!(body.contains("procFilter"), "dashboard must filter processes as you type");
    assert!(body.contains("tickscroll"), "ticker must animate");
    assert!(body.contains("X-API-Key"), "dashboard must send the API key header");
    assert!(body.contains("glances_key"), "dashboard must prompt for and store the API key");
    assert!(body.contains("id=\"healthdot\""), "dashboard must render the header health dot");
    assert!(body.contains("id=\"pubip\""), "dashboard must render the public IP row");
    assert!(body.contains("ip_addresses"), "dashboard must show per-interface IPs");
    assert!(body.contains("\"addr\""),
        "dashboard must render address rows aligned with the rate rows");
    assert!(body.contains("<button id=\"theme\""), "dashboard must render the theme cycle button");
    assert!(body.contains("[data-theme=\"1982\"]"), "dashboard must ship the 1982 theme");
    assert!(body.contains("--bg: #2e2015"), "1982 theme must use the woodgrain palette");
    assert!(body.contains("[data-theme=\"1992\"]"), "dashboard must ship the 1992 theme");
    assert!(body.contains("--bg: #16121a"), "1992 theme must use the boot-black palette");
    assert!(body.contains("[data-theme=\"2002\"]"), "dashboard must ship the 2002 theme");
    assert!(body.contains("--bg: #eaf0f6"), "2002 theme must use the optic-white palette");
    assert!(body.contains("[data-theme=\"2022\"]"), "dashboard must ship the 2022 theme");
    assert!(body.contains("--bg: #ece3d2"), "2022 theme must use the oat-milk palette");
    assert!(body.contains("const THEMES = [\"1982\", \"1992\", \"2002\", \"2022\"]"),
        "theme rotation must hold exactly the four year themes");
    assert!(!body.contains("banana") && !body.contains("Banana"),
        "dashboard must not ship the retired banana theme");
    assert!(!body.contains("tn-dark"), "dashboard must not ship the retired dark theme");
    assert!(body.contains("tn-1982") && body.contains("tn-1992") && body.contains("tn-2002") && body.contains("tn-2022"),
        "theme button must preview the next theme's colors");
    assert!(body.contains("glances_theme"), "dashboard must persist the theme choice");
    assert!(body.contains("width: 9ch"), "theme button must hold an exact fixed width");
    assert!(body.find("<button id=\"theme\"").unwrap() > body.find("id=\"state\"").unwrap(),
        "theme button must sit last in the header");
    assert!(body.contains("padding: 12px 16px 32px"), "content must keep gutters on all sides");
    assert!(body.contains("main > div { min-width: 0; }"),
        "grid columns must not force the page past the screen edge");
    assert!(body.contains("overflow-x: clip"), "page must never scroll sideways");
}

#[test]
fn dashboard_groups_sections_by_story() {
    let stats = GlancesStats::new(2.0);
    plugins::register_all(&stats);
    let args = Args { mode: Mode::WebServer, ..Args::default() };
    let ctx = test_ctx(&stats, &args);
    let req = Request { method: "GET".into(), path: "/dashboard".into(),
                        query: String::new(), version: "HTTP/1.1".into(),
                        headers: Default::default(), body: vec![] };
    let body = String::from_utf8(route(&req, &ctx).body).unwrap();
    // Health, engine, ledger, data — in that page order.
    let needles = ["id=\"warnings\"", "id=\"alerts\"", "id=\"sensors\"",
        "id=\"cpu-bar\"", "id=\"mem-bar\"", "id=\"power-total-h\"",
        "id=\"plist\"", "id=\"gpus\"", "id=\"spark-net\"", "id=\"conns\"",
        "id=\"diskio\"", "id=\"fs\""];
    let mut prev = 0;
    for n in needles {
        let at = body.find(n).unwrap_or_else(|| panic!("dashboard must render {n}"));
        assert!(at > prev, "{n} is out of story order");
        prev = at;
    }
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
