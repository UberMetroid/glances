//! Computed health rollup (`GET /api/4/health`).
//!
//! One status for eyes and machines: worst-of rollup over recent
//! alerts plus explicit checks (filesystems, memory, swap, GPUs,
//! temperatures, RAID, SMART). Checks only cover plugins reporting
//! data; missing data emits no check rather than a false ok.
//!
//! Thresholds (mirrored in docs/api.md):
//!   fs / memory percent: warn >= 90, critical >= 95
//!   swap percent: warn >= 50, critical >= 90
//!   gpu temp C: warn >= 80, critical >= 90
//!   sensor temp C: warn >= 85, critical >= 95
//!   alerts: CRITICAL -> critical, WARNING/CAREFUL -> warning
//!     (only entries from the last 60s count; older ones fade)
//!   raid: any failed -> critical, degraded/offline -> warning
//!   smart: attribute value <= threshold (threshold > 0) -> critical

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::response::Response;
use super::router::Ctx;
use crate::core::value::{self, Value};

const OK: u8 = 0;
const WARN: u8 = 1;
const CRIT: u8 = 2;

fn band(v: f64, warn_at: f64, crit_at: f64) -> u8 {
    if v >= crit_at { CRIT } else if v >= warn_at { WARN } else { OK }
}

fn word(rank: u8) -> &'static str {
    match rank {
        2 => "critical",
        1 => "warning",
        _ => "ok",
    }
}

fn num(obj: &BTreeMap<String, Value>, key: &str) -> Option<f64> {
    obj.get(key).and_then(Value::as_f64)
}

fn word_field<'a>(obj: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

/// One check: (rank, name, detail).
type Check = (u8, String, String);

/// Only alerts newer than this count toward the rollup. A sustained
/// breach re-logs every tick so it stays lit; an ended one fades on
/// its own without a manual clear. 60s spans two 30s idle ticks, so
/// a sustained idle breach cannot flap between checks.
const ALERT_RECENCY_SECS: f64 = 60.0;

fn now_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

pub(crate) fn alert_check(stats: &Value, out: &mut Vec<Check>) {
    let empty = Vec::new();
    let list = stats.as_array().unwrap_or(&empty);
    let now = now_unix();
    let (mut crit, mut warn) = (0, 0);
    for a in list {
        let Some(o) = a.as_object() else { continue };
        // Unknown age counts: only a positively-stale entry fades, so
        // a malformed record can never hide a live alert.
        if let Some(ts) = num(o, "timestamp")
            && now - ts > ALERT_RECENCY_SECS
        {
            continue;
        }
        match word_field(o, "type") {
            Some("CRITICAL") => crit += 1,
            Some("WARNING") | Some("CAREFUL") => warn += 1,
            _ => {}
        }
    }
    let rank = if crit > 0 { CRIT } else if warn > 0 { WARN } else { OK };
    let detail = match (crit, warn) {
        (0, 0) => "no active alerts".to_string(),
        (c, 0) => format!("{c} critical"),
        (0, w) => format!("{w} warning"),
        (c, w) => format!("{c} critical · {w} warning"),
    };
    out.push((rank, "alerts".to_string(), detail));
}

fn fs_checks(stats: &Value, out: &mut Vec<Check>) {
    let empty = Vec::new();
    for f in stats.as_array().unwrap_or(&empty) {
        let Some(o) = f.as_object() else { continue };
        let (Some(pct), Some(mnt)) = (num(o, "percent"), word_field(o, "mnt_point")) else { continue };
        out.push((band(pct, 90.0, 95.0), format!("fs:{mnt}"), format!("{pct:.1}% used")));
    }
}

fn pct_check(stats: &Value, name: &str, warn_at: f64, crit_at: f64, out: &mut Vec<Check>) {
    let Some(pct) = stats.as_object().and_then(|o| num(o, "percent")) else { return };
    out.push((band(pct, warn_at, crit_at), name.to_string(), format!("{pct:.1}% used")));
}

fn gpu_checks(stats: &Value, out: &mut Vec<Check>) {
    let empty = Vec::new();
    for (i, g) in stats.as_array().unwrap_or(&empty).iter().enumerate() {
        let Some(o) = g.as_object() else { continue };
        let Some(t) = num(o, "temp_c") else { continue };
        let id = word_field(o, "gpu_id").map(str::to_string).unwrap_or_else(|| format!("card{i}"));
        out.push((band(t, 80.0, 90.0), format!("gpu:{id}"), format!("{t:.0}°C")));
    }
}

fn temperature_check(stats: &Value, out: &mut Vec<Check>) {
    let empty = Vec::new();
    let mut best: Option<(f64, String)> = None;
    for s in stats.as_array().unwrap_or(&empty) {
        let Some(o) = s.as_object() else { continue };
        if word_field(o, "kind") != Some("temperature_c") { continue; }
        let Some(v) = num(o, "value") else { continue };
        let label = word_field(o, "label").or_else(|| word_field(o, "chip")).unwrap_or("sensor");
        if best.as_ref().is_none_or(|(b, _)| v > *b) {
            best = Some((v, label.to_string()));
        }
    }
    if let Some((v, label)) = best {
        out.push((band(v, 85.0, 95.0), "temperature".to_string(), format!("max {v:.0}°C ({label})")));
    }
}

fn raid_checks(stats: &Value, out: &mut Vec<Check>) {
    let empty = Vec::new();
    for r in stats.as_array().unwrap_or(&empty) {
        let Some(o) = r.as_object() else { continue };
        let name = word_field(o, "raid_name").unwrap_or("?");
        let failed = num(o, "failed").unwrap_or(0.0);
        let working = num(o, "working").unwrap_or(0.0);
        let total = num(o, "total").unwrap_or(0.0);
        let status = word_field(o, "status").unwrap_or("?");
        let (rank, detail) = if failed > 0.0 {
            (CRIT, format!("{failed:.0} failed · {working:.0}/{total:.0} working"))
        } else if working < total {
            (WARN, format!("degraded {working:.0}/{total:.0} working"))
        } else if status != "active" {
            (WARN, format!("status {status}"))
        } else {
            (OK, format!("{working:.0}/{total:.0} {status}"))
        };
        out.push((rank, format!("raid:{name}"), detail));
    }
}

fn smart_checks(stats: &Value, out: &mut Vec<Check>) {
    let empty = Vec::new();
    for d in stats.as_array().unwrap_or(&empty) {
        let Some(o) = d.as_object() else { continue };
        let name = word_field(o, "DeviceName").unwrap_or("?");
        let mut failing: Vec<String> = Vec::new();
        if let Some(attrs) = o.get("attributes").and_then(Value::as_array) {
            for a in attrs {
                let Some(ao) = a.as_object() else { continue };
                if let (Some(v), Some(t)) = (num(ao, "value"), num(ao, "threshold"))
                    && t > 0.0 && v <= t {
                        failing.push(word_field(ao, "name").unwrap_or("?").to_string());
                    }
            }
        }
        if failing.is_empty() {
            out.push((OK, format!("smart:{name}"), "attributes ok".to_string()));
        } else {
            out.push((CRIT, format!("smart:{name}"), format!("failing: {}", failing.join(", "))));
        }
    }
}

pub(crate) fn serve_health(ctx: &Ctx<'_>) -> Response {
    Response::ok_json(value::to_json(&health_value(ctx)))
}

/// The health rollup as a value, shared by `/api/4/health` and the
/// dashboard bundle route (which embeds it under `"health"`).
pub(crate) fn health_value(ctx: &Ctx<'_>) -> Value {
    let guard = ctx.stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let get = |name: &str| guard.iter().find(|p| p.name() == name).map(|p| p.stats());
    let mut checks: Vec<Check> = Vec::new();
    if let Some(v) = get("alert") { alert_check(v, &mut checks); }
    if let Some(v) = get("fs") { fs_checks(v, &mut checks); }
    if let Some(v) = get("mem") { pct_check(v, "memory", 90.0, 95.0, &mut checks); }
    if let Some(v) = get("memswap") { pct_check(v, "swap", 50.0, 90.0, &mut checks); }
    if let Some(v) = get("gpu") { gpu_checks(v, &mut checks); }
    if let Some(v) = get("sensors") { temperature_check(v, &mut checks); }
    if let Some(v) = get("raid") { raid_checks(v, &mut checks); }
    if let Some(v) = get("smart") { smart_checks(v, &mut checks); }
    // Worst first so eyes and machines see problems without scanning.
    checks.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let rank = checks.iter().map(|c| c.0).max().unwrap_or(OK);
    let nc = checks.iter().filter(|c| c.0 == CRIT).count();
    let nw = checks.iter().filter(|c| c.0 == WARN).count();
    let summary = match (nc, nw) {
        (0, 0) => "ALL SYSTEMS NOMINAL".to_string(),
        (c, 0) => format!("{c} critical"),
        (0, w) => format!("{w} warning"),
        (c, w) => format!("{c} critical · {w} warning"),
    };
    let arr: Vec<Value> = checks.into_iter().map(|(r, n, d)| {
        let mut m = BTreeMap::new();
        m.insert("name".to_string(), Value::String(n));
        m.insert("status".to_string(), Value::String(word(r).to_string()));
        m.insert("detail".to_string(), Value::String(d));
        Value::Object(m)
    }).collect();
    let mut top = BTreeMap::new();
    top.insert("status".to_string(), Value::String(word(rank).to_string()));
    top.insert("summary".to_string(), Value::String(summary));
    top.insert("checks".to_string(), Value::Array(arr));
    Value::Object(top)
}
