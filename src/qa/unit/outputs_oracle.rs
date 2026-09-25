//! Independent oracle for rewritten outputs: hand-computed wire
//! shapes, an empty-registry routing matrix, and auth-gate integration.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::cli::args::Args;
use crate::core::password::PasswordFile;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::outputs::web::request::Request;
use crate::outputs::web::router::{route, Ctx};

fn obj(pairs: &[(&str, Value)]) -> Value {
    Value::Object(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

fn req(method: &str, path: &str) -> Request {
    Request { method: method.into(), path: path.into(), query: String::new(),
              version: "HTTP/1.1".into(), headers: Default::default(), body: vec![] }
}

fn open_ctx<'a>(stats: &'a GlancesStats, args: &'a Args, pw: &'a PasswordFile) -> Ctx<'a> {
    Ctx { stats, args, password: pw, auth_enabled: false, api_key: None,
          refresh_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)) }
}

#[test]
fn csv_rows_match_hand_written_shapes() {
    use crate::outputs::csv_stdout::render_rows;
    let snap = obj(&[
        ("cpu", obj(&[("total", Value::Float(50.5))])),
        ("note", Value::String("a,b".into())),
    ]);
    assert_eq!(
        render_rows(&snap, 1.0),
        vec![
            "1.000,cpu,total,50.50,,".to_string(),
            "1.000,note,,\"a,b\",,".to_string(),
        ]
    );
}

#[test]
fn json_line_matches_hand_written_shape() {
    use crate::outputs::json_stdout::render_line;
    let snap = obj(&[
        ("cpu", obj(&[("total", Value::Float(1.5))])),
        ("up", Value::Bool(true)),
        ("missing", Value::Null),
    ]);
    assert_eq!(
        render_line(&snap, 1.25),
        "{\"timestamp\":1.25,\"plugins\":{\"cpu\":{\"total\":1.5},\"missing\":null,\"up\":true}}"
    );
}

#[test]
fn empty_registry_routing_matrix() {
    let stats = GlancesStats::new(2.0);
    let args = Args::default();
    let pw = PasswordFile::empty();
    let ctx = open_ctx(&stats, &args, &pw);
    let get = |p: &str| route(&req("GET", p), &ctx);
    assert_eq!(get("/healthz").status, 200);
    assert_eq!(String::from_utf8_lossy(&get("/healthz").body), "ok\n");
    assert_eq!(get("/nope").status, 404);
    assert_eq!(get("/api/4/cpu").status, 404);
    assert_eq!(get("/api/4/nonexistent").status, 404);
    assert_eq!(get("/api/4/cpu/total").status, 404);
    assert_eq!(get("/api/4/history").status, 200);
    assert_eq!(get("/api/4/pluginslist").status, 200);
    assert_eq!(String::from_utf8_lossy(&get("/api/4/pluginslist").body), "[]");
    assert_eq!(get("/api/4/serverslist").status, 200);
    let status = get("/api/4/status");
    assert_eq!(status.status, 200);
    assert!(String::from_utf8_lossy(&status.body).contains("\"version\""));
    let clear = route(&req("POST", "/api/4/events/clear/all"), &ctx);
    assert_eq!((clear.status, String::from_utf8_lossy(&clear.body).as_ref()), (200, "{}"));
    assert_eq!(route(&req("POST", "/api/4/token"), &ctx).status, 501);
    assert_eq!(route(&req("DELETE", "/api/4/cpu"), &ctx).status, 404);
}

#[test]
fn gates_reject_and_keys_pass() {
    let stats = GlancesStats::new(2.0);
    let args = Args::default();
    let pw = PasswordFile::empty();
    let basic = Ctx { stats: &stats, args: &args, password: &pw, auth_enabled: true, api_key: None,
        refresh_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)) };
    assert_eq!(route(&req("GET", "/api/4/cpu"), &basic).status, 401);
    assert_eq!(route(&req("GET", "/favicon.ico"), &basic).status, 200);
    let keyed = Ctx { stats: &stats, args: &args, password: &pw, auth_enabled: false,
        api_key: Some("k".into()), refresh_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)) };
    assert_eq!(route(&req("GET", "/api/4/cpu"), &keyed).status, 401);
    let mut with_key = req("GET", "/healthz");
    with_key.headers.insert("x-api-key".into(), "k".into());
    assert_eq!(route(&with_key, &keyed).status, 200);
    let mut wrong = req("GET", "/healthz");
    wrong.headers.insert("x-api-key".into(), "nope".into());
    assert_eq!(route(&wrong, &keyed).status, 401);
}
