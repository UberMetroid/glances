//! Tests for the aggregate metadata endpoints (limits/views/history).

use crate::cli::args::{Args, Mode};
use crate::core::stats::GlancesStats;
use crate::outputs::web::request::Request;
use crate::outputs::web::router::{route, test_ctx};
use crate::plugins;

    #[test]
    fn limits_views_history_endpoints() {
        let stats = GlancesStats::new(2.0);
        plugins::register_all(&stats);
        stats.update().unwrap();
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let ctx = test_ctx(&stats, &args);
        let mk = |path: &str| Request { method: "GET".into(), path: path.into(),
                            query: String::new(), version: "HTTP/1.1".into(),
                            headers: Default::default(), body: vec![] };
        let limits = route(&mk("/api/all/limits"), &ctx);
        assert_eq!(limits.status, 200);
        let body = String::from_utf8(limits.body).unwrap();
        assert!(body.contains("\"cpu\""), "limits must name plugins: {}", &body[..body.len().min(200)]);
        let views = route(&mk("/api/all/views"), &ctx);
        assert_eq!(views.status, 200);
        let history = route(&mk("/api/4/history"), &ctx);
        assert_eq!(history.status, 200);
    }

    #[test]
    fn health_endpoint_rolls_up() {
        let stats = GlancesStats::new(2.0);
        plugins::register_all(&stats);
        stats.update().unwrap();
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let ctx = test_ctx(&stats, &args);
        let req = Request { method: "GET".into(), path: "/api/4/health".into(),
                            query: String::new(), version: "HTTP/1.1".into(),
                            headers: Default::default(), body: vec![] };
        let r = route(&req, &ctx);
        assert_eq!(r.status, 200);
        let body = String::from_utf8(r.body).unwrap();
        assert!(body.contains("\"status\"") && body.contains("\"checks\""),
                "health must roll up status + checks: {}", body.chars().take(200).collect::<String>());
    }
