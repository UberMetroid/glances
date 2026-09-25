//! Independent oracle for rewritten core: every expectation below is
//! derived from ground truth inside the test (hand-computed math, crafted
//! snapshots, real sleeps) — never copied from the implementation.

use std::collections::BTreeMap;

use crate::core::alerts::LimitValue;
use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::filter::Regex;
use crate::core::history::GlancesHistory;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::stats::GlancesStats;
use crate::core::threshold::{evaluate, Severity};
use crate::core::value::Value;

fn obj(pairs: &[(&str, f64)]) -> Value {
    Value::Object(pairs.iter().map(|(k, v)| ((*k).to_string(), Value::Float(*v))).collect())
}

#[test]
fn severity_bands_from_first_principles() {
    assert_eq!(evaluate(0.0, Some(50.0), Some(70.0), Some(90.0)), Severity::Ok);
    assert_eq!(evaluate(50.0, Some(50.0), Some(70.0), Some(90.0)), Severity::Careful);
    assert_eq!(evaluate(89.9, Some(50.0), Some(70.0), Some(90.0)), Severity::Warning);
    assert_eq!(evaluate(100.0, Some(50.0), Some(70.0), Some(90.0)), Severity::Critical);
    assert!(Severity::Ok < Severity::Critical);
}

#[test]
fn rate_is_delta_over_elapsed() {
    let mut m = GlancesPluginModel::new("t", obj(&[("bytes", 100.0)]));
    m.manage_rate();
    m.stats = obj(&[("bytes", 160.0)]);
    m.manage_rate();
    let o = m.stats.as_object().unwrap();
    let dt = o.get("time_since_update").and_then(Value::as_f64).unwrap();
    let rate = o.get("bytes_rate_per_sec").and_then(Value::as_f64).unwrap();
    assert!(dt > 0.0);
    assert!((rate - 60.0 / dt).abs() < 1e-6);
    assert_eq!(o.get("bytes_gauge").and_then(Value::as_f64), Some(160.0));
}

#[test]
fn mmm_tracks_exact_min_max_mean() {
    let mut m = GlancesPluginModel::new("t", obj(&[("v", 10.0)]));
    for v in [10.0, 20.0, 30.0] {
        m.stats = obj(&[("v", v)]);
        m.manage_mmm();
    }
    let o = m.stats.as_object().unwrap();
    assert_eq!(o.get("v_min").and_then(Value::as_f64), Some(10.0));
    assert_eq!(o.get("v_max").and_then(Value::as_f64), Some(30.0));
    assert_eq!(o.get("v_mean").and_then(Value::as_f64), Some(20.0));
}

#[test]
fn history_slope_needs_two_samples_and_time() {
    let mut h = GlancesHistory::new();
    assert_eq!(h.rate("s"), 0.0);
    h.add("s", 0.0);
    std::thread::sleep(std::time::Duration::from_millis(10));
    h.add("s", 10.0);
    let r = h.rate("s");
    assert!(r > 50.0 && r < 20000.0, "slope out of range: {r}");
}

#[test]
fn alert_words_from_hand_set_limits() {
    let mut m = GlancesPluginModel::new("mem", Value::Null);
    for (k, v) in [("mem_careful", 50.0), ("mem_warning", 70.0), ("mem_critical", 90.0)] {
        m.limits.insert(k.into(), LimitValue::Float(v));
    }
    assert_eq!(m.get_alert(95.0, 0.0, 100.0, "", None, false, true, None, None), "CRITICAL");
    assert_eq!(m.get_alert(75.0, 0.0, 100.0, "", None, false, true, None, None), "WARNING");
    assert_eq!(m.get_alert(55.0, 0.0, 100.0, "", None, false, true, None, None), "CAREFUL");
    assert_eq!(m.get_alert(5.0, 0.0, 100.0, "", None, false, true, None, None), "OK");
    assert_eq!(m.get_alert(0.0, 0.0, 100.0, "", None, false, false, None, None), "DEFAULT");
    assert_eq!(m.thresholds.get("mem").map(String::as_str), Some("OK"));
    let mut log = EventLog::default();
    let out = m.get_alert(95.0, 0.0, 100.0, "", None, false, true, Some(true), Some(&mut log));
    assert_eq!(out, "CRITICAL_LOG");
    assert_eq!(log.len(), 1);
}

#[test]
fn ctx_switches_defaults_scale_with_cores() {
    let mut m = GlancesPluginModel::new("cpu", Value::Null);
    m.apply_default_limits(&[("ctx_switches", 0.0, 0.0, 0.0)], 4);
    let full = 500_000.0 * 0.10 * 4.0;
    assert_eq!(m.limits.get("cpu_ctx_switches_careful"), Some(&LimitValue::Float(full * 0.80)));
    assert_eq!(m.limits.get("cpu_ctx_switches_critical"), Some(&LimitValue::Float(full)));
}

struct Stub {
    name: &'static str,
    stats: Value,
}

impl Plugin for Stub {
    fn name(&self) -> &'static str { self.name }
    fn reset(&mut self) {}
    fn update(&mut self) -> Result<()> { Ok(()) }
    fn stats(&self) -> &Value { &self.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.stats }
}

fn stub(name: &'static str, pairs: &[(&str, f64)]) -> Stub {
    Stub { name, stats: obj(pairs) }
}

#[test]
fn quicklook_math_from_synthetic_siblings() {
    let stats = GlancesStats::new(1.0);
    stats.register(Box::new(stub("cpu", &[("total", 25.0)])));
    stats.register(Box::new(stub("mem", &[("percent", 50.0)])));
    stats.register(Box::new(stub("memswap", &[("percent", 10.0)])));
    stats.register(Box::new(stub("load", &[("min1", 6.0), ("cpucore", 12.0)])));
    stats.register(Box::new(Stub { name: "quicklook", stats: Value::Object(BTreeMap::new()) }));
    stats.update().unwrap();
    let snap = stats.snapshot();
    let q = snap.as_object().unwrap().get("quicklook").unwrap().as_object().unwrap();
    assert_eq!(q.get("cpu").and_then(Value::as_f64), Some(25.0));
    assert_eq!(q.get("mem").and_then(Value::as_f64), Some(50.0));
    assert_eq!(q.get("swap").and_then(Value::as_f64), Some(10.0));
    assert_eq!(q.get("load").and_then(Value::as_f64), Some(50.0));
}

struct Boom {
    stats: Value,
}

impl Plugin for Boom {
    fn name(&self) -> &'static str { "boom" }
    fn reset(&mut self) {}
    fn update(&mut self) -> Result<()> { panic!("boom") }
    fn stats(&self) -> &Value { &self.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.stats }
}

#[test]
fn panicking_plugin_does_not_stop_the_tick() {
    let stats = GlancesStats::new(1.0);
    stats.register(Box::new(Boom { stats: obj(&[("x", 1.0)]) }));
    stats.register(Box::new(stub("mem", &[("percent", 42.0)])));
    stats.update().unwrap();
    let snap = stats.snapshot();
    let mem = snap.as_object().unwrap().get("mem").unwrap().as_object().unwrap();
    assert_eq!(mem.get("percent").and_then(Value::as_f64), Some(42.0));
}

#[test]
fn regex_matrix_and_termination() {
    let re = Regex::compile("a+b").unwrap();
    assert!(re.is_match("xxaab"));
    assert!(!re.is_match("xxac"));
    assert!(re.is_full_match("aaab"));
    assert!(!re.is_full_match("aaabc"));
    let anchored = Regex::compile("^ab$").unwrap();
    assert!(anchored.is_match("ab"));
    assert!(!anchored.is_match("xab"));
    let class = Regex::compile("[a-c]+[^0-9]").unwrap();
    assert!(class.is_full_match("abcx"));
    assert!(!class.is_full_match("abc5"));
    let alt = Regex::compile("(ab|cd)+").unwrap();
    assert!(alt.is_full_match("abcdab"));
    assert!(!alt.is_full_match("abce"));
    // Empty-matching repeats must terminate.
    assert!(Regex::compile("(a*)*").unwrap().is_match("b"));
    assert!(Regex::compile("(a*)*").unwrap().is_full_match(""));
}

#[test]
fn refused_template_records_trigger_but_writes_no_file() {
    use crate::core::actions::GlancesActions;
    let path = std::env::temp_dir().join("glances-rs-oracle-noref.txt");
    let _ = std::fs::remove_file(&path);
    let cmd = format!("echo {{{{#x}}}} > {}", path.display());
    let mut a = GlancesActions::new(0.0, true);
    assert!(a.run("s", "CRITICAL", &[cmd], true, &BTreeMap::new()));
    assert_eq!(a.get("s"), Some("CRITICAL"));
    assert!(!path.exists());
}

#[test]
fn credential_shapes_parse_by_prefix() {
    use crate::core::password::{PasswordFile, PasswordHash};
    let path = std::env::temp_dir().join("glances-rs-oracle-shapes");
    std::fs::write(
        &path,
        "# comment\n\nplain:abc123\nsalted:$sha256$ss$hh\nkdf:salt$hex\njunk-no-colon\n",
    )
    .unwrap();
    let pf = PasswordFile::load(&path).unwrap();
    assert!(matches!(pf.entries.get("plain"), Some(PasswordHash::Plain(_))));
    assert!(matches!(pf.entries.get("salted"), Some(PasswordHash::Salted { .. })));
    assert!(matches!(pf.entries.get("kdf"), Some(PasswordHash::Pbkdf2 { .. })));
    assert_eq!(pf.entries.len(), 3);
    let _ = std::fs::remove_file(path);
}

#[test]
fn kdf_output_spans_blocks_and_truncates() {
    use crate::core::pbkdf2::pbkdf2_hmac_sha256;
    assert!(pbkdf2_hmac_sha256(b"p", b"s", 1, 0).is_empty());
    assert_eq!(pbkdf2_hmac_sha256(b"p", b"s", 1, 32).len(), 32);
    assert_eq!(pbkdf2_hmac_sha256(b"p", b"s", 1, 64).len(), 64);
}

#[test]
fn config_paths_are_sensible_without_env_changes() {
    for p in crate::core::config_dir::candidate_paths() {
        assert_eq!(p.file_name().and_then(|s| s.to_str()), Some("glances.conf"));
    }
    assert_eq!(
        crate::core::config_dir::resolve(Some("/tmp/x.conf")),
        std::path::PathBuf::from("/tmp/x.conf")
    );
    assert!(crate::core::config_dir::user_dir().to_string_lossy().contains("glances"));
}
