//! Unit tests for the Prometheus text-exposition exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
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

#[test]
fn emits_help_type_and_sample_per_metric() {
    let snap = snap_of("cpu", &[("total", Value::Float(12.5))]);
    let cfg = prometheus::Config {
        prefix: "glances".into(),
        include_timestamp: false,
        timestamp: Some(1_700_000_000.0),
        ..Default::default()
    };
    let sink = prometheus::WriterSink::in_memory();
    prometheus::write_to(&snap, &cfg, &sink).expect("write");
    let out = sink.into_string();
    assert!(out.contains("# HELP glances_cpu_total cpu total\n"));
    assert!(out.contains("# TYPE glances_cpu_total gauge\n"));
    // include_timestamp=false → no trailing timestamp.
    assert!(out.contains("glances_cpu_total 12.5\n"));
}

#[test]
fn timestamp_suffix_when_enabled() {
    let snap = snap_of("mem", &[("used", Value::Int(2048))]);
    let cfg = prometheus::Config {
        prefix: "gl".into(),
        include_timestamp: true,
        timestamp: Some(1_700_000_000.0),
        ..Default::default()
    };
    let sink = prometheus::WriterSink::in_memory();
    prometheus::write_to(&snap, &cfg, &sink).expect("write");
    let out = sink.into_string();
    // Timestamp is rendered as i64 seconds.
    assert!(out.contains("gl_mem_used 2048 1700000000\n"));
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
    let sink = prometheus::WriterSink::in_memory();
    prometheus::write_to(&snap, &cfg, &sink).expect("write");
    let out = sink.into_string();
    assert!(out.contains("gl_cpu_bad NaN\n"));
}

#[test]
fn unicode_in_names_is_sanitized() {
    // Plugin name "café" becomes "caf_" after sanitize (é is non-ASCII
    // alphanumeric → kept verbatim; spaces become '_').
    let snap = obj(&[("café cpu", obj(&[("naïve", Value::Int(1))]))]);
    let cfg = prometheus::Config {
        prefix: "gl".into(),
        include_timestamp: false,
        timestamp: Some(0.0),
        ..Default::default()
    };
    let sink = prometheus::WriterSink::in_memory();
    prometheus::write_to(&snap, &cfg, &sink).expect("write");
    let out = sink.into_string();
    // The space in plugin name is replaced with '_'.
    assert!(out.contains("gl_café_cpu_naïve"), "got: {}", out);
    assert!(out.contains("1\n"));
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
    let sink = prometheus::WriterSink::in_memory();
    prometheus::write_to(&snap, &cfg, &sink).expect("write");
    let out = sink.into_string();
    assert!(out.contains("gl_mem_used 100\n"));
    assert!(!out.contains("gl_mem_name"));
    assert!(!out.contains("gl_mem_flag"));
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
    let sink = prometheus::WriterSink::in_memory();
    prometheus::write_to(&snap, &cfg, &sink).expect("write");
    assert_eq!(sink.into_string(), "");
}