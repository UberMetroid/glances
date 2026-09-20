//! POST mutators + the extended-process getter (upstream
//! `glances_restful_api.py` POST block parity).
//!
//! * `POST /api/4/events/clear/warning|all` → `EventLog::clean`
//! * `POST /api/4/processes/extended/{pid}|disable` → pin/clear pid
//! * `GET /api/4/processes/extended` → pinned pid's stats or `null`

use super::response::Response;
use super::router::Ctx;
#[cfg(test)]
use super::request::Request;
#[cfg(test)]
use super::router::{route, test_ctx};
use crate::core::value::{self, Value};

/// Find a processlist entry by pid. Returns a clone so the plugin
/// lock is never held across the response build.
fn find_process(ctx: &Ctx<'_>, pid: u32) -> Option<Value> {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let p = guard.iter().find(|p| p.name() == "processlist")?;
    p.stats().as_array()?.iter().find(|v| {
        v.as_object()
            .and_then(|o| o.get("pid"))
            .and_then(|v| v.as_f64())
            .is_some_and(|n| n as u32 == pid)
    }).cloned()
}

/// `POST /api/4/events/clear/warning|all` — clean the event log.
pub fn clear_events(ctx: &Ctx<'_>, critical: bool) -> Response {
    if let Ok(mut ev) = ctx.stats.events.lock() { ev.clean(critical); }
    Response::ok_json("{}".into())
}

/// `POST /api/4/processes/extended/disable` — clear the pinned pid.
pub fn disable_extended(ctx: &Ctx<'_>) -> Response {
    if let Ok(mut ext) = ctx.stats.extended_process.lock() { *ext = None; }
    Response::ok_json("true".into())
}

/// `POST /api/4/processes/extended/{pid}` — pin extended stats.
/// 200 `true` when the pid exists, 404 for unknown pids, 400 for a
/// non-numeric pid (upstream `_api_set_extended_processes` parity).
pub fn serve_set_extended_process(path: &str, ctx: &Ctx<'_>) -> Response {
    let pid: u32 = match path.rsplit('/').next().unwrap_or("").parse() {
        Ok(n) => n,
        Err(_) => return Response::bad_request("pid must be numeric"),
    };
    match find_process(ctx, pid) {
        Some(_) => {
            if let Ok(mut ext) = ctx.stats.extended_process.lock() { *ext = Some(pid); }
            Response::ok_json("true".into())
        }
        None => Response::not_found(),
    }
}

/// `GET /api/4/processes/extended` — pinned pid's stats, or `null`
/// (upstream `_api_get_extended_processes` parity).
pub fn serve_extended_process(ctx: &Ctx<'_>) -> Response {
    let pid = ctx.stats.extended_process.lock().ok().and_then(|g| *g);
    match pid.and_then(|n| find_process(ctx, n)) {
        Some(v) => Response::ok_json(value::to_json(&v)),
        None => Response::ok_json("null".into()),
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

    fn live_stats() -> GlancesStats {
        let stats = GlancesStats::new(2.0);
        plugins::register_all(&stats);
        // Real /proc sampling: our own test process must show up.
        let _ = stats.update();
        stats
    }

    #[test]
    fn events_clear_warning_keeps_critical() {
        let stats = live_stats();
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let ctx = test_ctx(&stats, &args);
        let push = |sev| {
            stats.events.lock().unwrap().push(Event {
                severity: sev, stat: "cpu".into(), value: 99.0,
                timestamp: std::time::SystemTime::now(),
            });
        };
        push(Severity::Warning);
        push(Severity::Critical);
        let r = route(&post("/api/4/events/clear/warning"), &ctx);
        assert_eq!(r.status, 200);
        assert_eq!(String::from_utf8_lossy(&r.body), "{}");
        let kept: Vec<Severity> = stats.events.lock().unwrap()
            .snapshot().iter().map(|e| e.severity).collect();
        assert_eq!(kept, vec![Severity::Critical]);
        let r = route(&post("/api/4/events/clear/all"), &ctx);
        assert_eq!(r.status, 200);
        assert!(stats.events.lock().unwrap().is_empty());
    }

    #[test]
    fn extended_pin_round_trip() {
        let stats = live_stats();
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let ctx = test_ctx(&stats, &args);
        let me = std::process::id();
        let r = route(&post(&format!("/api/4/processes/extended/{}", me)), &ctx);
        assert_eq!(r.status, 200, "own pid must pin");
        assert_eq!(String::from_utf8_lossy(&r.body), "true");
        let get = Request { method: "GET".into(),
            path: "/api/4/processes/extended".into(), query: String::new(),
            version: "HTTP/1.1".into(), headers: Default::default(), body: vec![] };
        let r = route(&get, &ctx);
        assert_eq!(r.status, 200);
        assert!(String::from_utf8_lossy(&r.body).contains(&me.to_string()));
        let r = route(&post("/api/4/processes/extended/disable"), &ctx);
        assert_eq!(r.status, 200);
        let r = route(&get, &ctx);
        assert_eq!(String::from_utf8_lossy(&r.body), "null");
    }

    #[test]
    fn extended_unknown_and_bad_pid() {
        let stats = live_stats();
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let ctx = test_ctx(&stats, &args);
        assert_eq!(route(&post("/api/4/processes/extended/4294967295"), &ctx).status, 404);
        assert_eq!(route(&post("/api/4/processes/extended/nope"), &ctx).status, 400);
    }

    #[test]
    fn token_is_501() {
        let stats = live_stats();
        let args = Args { mode: Mode::WebServer, ..Args::default() };
        let ctx = test_ctx(&stats, &args);
        assert_eq!(route(&post("/api/4/token"), &ctx).status, 501);
    }
}
