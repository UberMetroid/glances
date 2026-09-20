//! Unit tests for the Prometheus text-exposition exporter.
//!
//! `write()` also spawns a scrape listener and can append to a file —
//! these tests use `render()` to capture the exposition without side
//! effects.

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::prometheus;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

fn snap_of(plugin: &str, pairs: &[(&str, Value)]) -> Value {
    obj(&[(plugin, obj(pairs))])
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
}

fn render_string(fields: &[Field<'_>], cfg: &prometheus::Config, ts_suffix: &str) -> String {
    let mut buf = Vec::new();
    prometheus::render(fields, cfg, &mut buf, ts_suffix).expect("render");
    String::from_utf8(buf).unwrap()
}

#[test]
fn emits_help_type_and_sample_per_metric() {
    let snap = snap_of("cpu", &[("total", Value::Float(12.5))]);
    let cfg = prometheus::Config {
        prefix: "glances".into(),
        include_timestamp: false,
        timestamp: Some(1_700_000_000.0),
        ..Default::default()
    };
    let out = render_string(&flat(&snap), &cfg, "");
    assert!(out.contains("# HELP glances_cpu_total cpu.total\n"), "got: {}", out);
    assert!(out.contains("# TYPE glances_cpu_total gauge\n"));
    // Empty ts suffix → no trailing timestamp.
    assert!(out.contains("glances_cpu_total{src=\"glances\"} 12.5\n"));
}

#[test]
fn timestamp_suffix_is_milliseconds() {
    let snap = snap_of("mem", &[("used", Value::Int(2048))]);
    let cfg = prometheus::Config {
        prefix: "gl".into(),
        include_timestamp: true,
        timestamp: Some(1_700_000_000.0),
        ..Default::default()
    };
    // Exposition timestamps are unix milliseconds (1.7e9 s → 1.7e12 ms).
    let out = render_string(&flat(&snap), &cfg, " 1700000000000");
    assert!(out.contains("gl_mem_used{src=\"glances\"} 2048.0 1700000000000\n"), "got: {}", out);
}

#[test]
fn nan_renders_as_nan_string() {
    let snap = snap_of("cpu", &[("bad", Value::Float(f64::NAN))]);
    let cfg = prometheus::Config {
        prefix: "gl".into(),
        include_timestamp: false,
        timestamp: Some(0.0),
        ..Default::default()
    };
    let out = render_string(&flat(&snap), &cfg, "");
    assert!(out.contains("gl_cpu_bad{src=\"glances\"} NaN\n"));
}

#[test]
fn unicode_in_names_is_sanitized() {
    // Metric charset is [a-zA-Z0-9_:] — non-ASCII (é, ï) and spaces
    // each become a single '_'.
    let snap = obj(&[("café cpu", obj(&[("naïve", Value::Int(1))]))]);
    let cfg = prometheus::Config {
        prefix: "gl".into(),
        include_timestamp: false,
        timestamp: Some(0.0),
        ..Default::default()
    };
    let out = render_string(&flat(&snap), &cfg, "");
    assert!(out.contains("gl_caf__cpu_na_ve{src=\"glances\"} 1.0\n"), "got: {}", out);
}

#[test]
fn non_numeric_values_are_skipped() {
    let snap = snap_of("mem", &[
        ("used", Value::Int(100)),
        ("name", Value::String("x".into())),
        ("flag", Value::Bool(true)),
    ]);
    let cfg = prometheus::Config {
        prefix: "gl".into(),
        include_timestamp: false,
        timestamp: Some(0.0),
        ..Default::default()
    };
    let out = render_string(&flat(&snap), &cfg, "");
    assert!(out.contains("gl_mem_used{src=\"glances\"} 100.0\n"));
    assert!(!out.contains("gl_mem_name"));
    // Upstream converts booleans with float(): flag is emitted as 1.0.
    assert!(out.contains("gl_mem_flag{src=\"glances\"} 1.0\n"), "got: {}", out);
}

#[test]
fn empty_snapshot_emits_nothing() {
    let snap = Value::Object(BTreeMap::new());
    let cfg = prometheus::Config {
        prefix: "gl".into(),
        include_timestamp: false,
        timestamp: Some(0.0),
        ..Default::default()
    };
    assert_eq!(render_string(&flat(&snap), &cfg, ""), "");
}
