//! State-changing endpoints: event clears, extended-process pinning,
//! and single-process lookup.
//!
//! * `POST /api/4/events/clear/warning|all` prunes the event log
//! * `POST /api/4/processes/extended/{pid}|disable` pins/clears a pid
//! * `GET /api/4/processes/extended` reads the pinned pid's stats
//! * `GET /api/4/processes/{pid}` reads one process by pid

use super::response::Response;
use super::router::Ctx;
#[cfg(test)]
use super::request::Request;
#[cfg(test)]
use super::router::{route, test_ctx};
use crate::core::value::{self, Value};

/// One processlist entry by pid, cloned so no lock is held while the
/// response builds.
fn find_process(ctx: &Ctx<'_>, pid: u32) -> Option<Value> {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    guard
        .iter()
        .find(|p| p.name() == "processlist")?
        .stats()
        .as_array()?
        .iter()
        .find(|v| {
            v.as_object()
                .and_then(|o| o.get("pid"))
                .and_then(Value::as_f64)
                .is_some_and(|n| n as u32 == pid)
        })
        .cloned()
}

/// `POST …/events/clear/…` — prune the log (warning bands, or all).
pub fn clear_events(ctx: &Ctx<'_>, critical: bool) -> Response {
    if let Ok(mut ev) = ctx.stats.events.lock() { ev.clean(critical); }
    Response::ok_json("{}".into())
}

/// `POST …/processes/extended/disable` — unpin.
pub fn disable_extended(ctx: &Ctx<'_>) -> Response {
    if let Ok(mut ext) = ctx.stats.extended_process.lock() { *ext = None; }
    Response::ok_json("true".into())
}

/// `POST …/processes/extended/{pid}` — pin when the pid exists (200
/// `true`), 404 for ghosts, 400 for non-numeric pids.
pub fn serve_set_extended_process(path: &str, ctx: &Ctx<'_>) -> Response {
    let Ok(pid): Result<u32, _> = path.rsplit('/').next().unwrap_or("").parse() else {
        return Response::bad_request("pid must be numeric");
    };
    match find_process(ctx, pid) {
        Some(_) => {
            if let Ok(mut ext) = ctx.stats.extended_process.lock() { *ext = Some(pid); }
            Response::ok_json("true".into())
        }
        None => Response::not_found(),
    }
}

/// `GET …/processes/extended` — pinned pid's stats, or `{}`.
pub fn serve_extended_process(ctx: &Ctx<'_>) -> Response {
    let found = ctx.stats.extended_process.lock().ok().and_then(|g| *g).and_then(|n| find_process(ctx, n));
    match found {
        Some(v) => Response::ok_json(value::to_json(&v)),
        None => Response::ok_json("{}".into()),
    }
}

/// `GET /api/4/processes/{pid}` — one process dict; 404 when absent or
/// non-numeric.
pub fn serve_process_by_pid(path: &str, ctx: &Ctx<'_>) -> Response {
    let Ok(pid): Result<u32, _> = path.rsplit('/').next().unwrap_or("").parse() else {
        return Response::not_found();
    };
    match find_process(ctx, pid) {
        Some(v) => Response::ok_json(value::to_json(&v)),
        None => Response::not_found(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Args, Mode};
    use crate::core::events::Event;
    use crate::core::stats::GlancesStats;
    use crate::core::threshold::Severity;
    use crate::plugins;

    fn post(path: &str) -> Request {
        Request { method: "POST".into(), path: path.into(), query: String::new(),
                 version: "HTTP/1.1".into(), headers: Default::default(), body: vec![] }
    }

    fn get(path: &str) -> Request {
        Request { method: "GET".into(), path: path.into(), query: String::new(),
                 version: "HTTP/1.1".into(), headers: Default::default(), body: vec![] }
    }

    fn live_stats() -> GlancesStats {
        let stats = GlancesStats::new(2.0);
        plugins::register_all(&stats);
        let _ = stats.update();
        stats
    }

    #[test]
    fn warning_clear_keeps_criticals_all_empties() {
        let stats = live_stats();
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let ctx = test_ctx(&stats, &args);
        for sev in [Severity::Warning, Severity::Critical] {
            stats.events.lock().unwrap().push(Event {
                severity: sev, stat: "cpu".into(), value: 99.0,
                timestamp: std::time::SystemTime::now(),
            });
        }
        let r = route(&post("/api/4/events/clear/warning"), &ctx);
        assert_eq!((r.status, String::from_utf8_lossy(&r.body).as_ref()), (200, "{}"));
        let kept: Vec<Severity> =
            stats.events.lock().unwrap().snapshot().iter().map(|e| e.severity).collect();
        assert_eq!(kept, vec![Severity::Critical]);
        let r = route(&post("/api/4/events/clear/all"), &ctx);
        assert_eq!(r.status, 200);
        assert!(stats.events.lock().unwrap().is_empty());
    }

    #[test]
    fn pin_read_unpin_round_trip() {
        let stats = live_stats();
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let ctx = test_ctx(&stats, &args);
        let me = std::process::id();
        let r = route(&post(&format!("/api/4/processes/extended/{me}")), &ctx);
        assert_eq!(r.status, 200, "own pid must pin");
        assert_eq!(String::from_utf8_lossy(&r.body), "true");
        let r = route(&get("/api/4/processes/extended"), &ctx);
        assert_eq!(r.status, 200);
        assert!(String::from_utf8_lossy(&r.body).contains(&me.to_string()));
        assert_eq!(route(&post("/api/4/processes/extended/disable"), &ctx).status, 200);
        let r = route(&get("/api/4/processes/extended"), &ctx);
        assert_eq!(String::from_utf8_lossy(&r.body), "{}");
    }

    #[test]
    fn ghost_and_garbage_pids() {
        let stats = live_stats();
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let ctx = test_ctx(&stats, &args);
        assert_eq!(route(&post("/api/4/processes/extended/4294967295"), &ctx).status, 404);
        assert_eq!(route(&post("/api/4/processes/extended/nope"), &ctx).status, 400);
        assert_eq!(route(&post("/api/4/token"), &ctx).status, 501);
    }
}
