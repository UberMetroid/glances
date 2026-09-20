//! Unit tests for the graph SVG exporter.

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::graph;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
}

fn cfg() -> graph::Config {
    graph::Config { path: "graphs".into(), width: 200, height: 150, max_points: 300 }
}

#[test]
fn append_fields_accumulates_numeric_only() {
    let snap = obj(&[(
        "cpu",
        obj(&[("total", Value::Float(10.0)), ("label", Value::String("x".into()))]),
    )]);
    let fields = flat(&snap);
    let mut map = BTreeMap::new();
    graph::append_fields(&mut map, &fields, 1000.0, 300);
    graph::append_fields(&mut map, &fields, 1001.0, 300);
    assert_eq!(map.len(), 1);
    assert_eq!(map["cpu.total"], vec![(1000.0, 10.0), (1001.0, 10.0)]);
}

#[test]
fn append_fields_caps_series_length() {
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(1.0))]))]);
    let fields = flat(&snap);
    let mut map = BTreeMap::new();
    for i in 0..10 {
        graph::append_fields(&mut map, &fields, i as f64, 4);
    }
    assert_eq!(map["cpu.total"].len(), 4);
    assert_eq!(map["cpu.total"][3], (9.0, 1.0));
}

#[test]
fn subsample_keeps_latest() {
    let pts: Vec<(f64, f64)> = (0..100).map(|i| (i as f64, i as f64)).collect();
    let sub = graph::subsample(&pts, 10);
    assert!(sub.len() <= 11);
    assert_eq!(*sub.last().unwrap(), (99.0, 99.0));
}

#[test]
fn render_svg_is_well_formed() {
    let series = vec![("total".to_string(), vec![(1.0, 10.0), (2.0, 20.0), (3.0, 15.0)])];
    let svg = graph::render_svg("cpu", &series, 200, 150);
    assert!(svg.starts_with("<svg "));
    assert!(svg.contains("<polyline "));
    assert!(svg.contains("total"));
    assert!(svg.trim_end().ends_with("</svg>"));
}

#[test]
fn render_svg_escapes_titles() {
    let series = vec![("a&b".to_string(), vec![(1.0, 1.0)])];
    let svg = graph::render_svg("x<y>", &series, 200, 150);
    assert!(svg.contains("x&lt;y&gt;"));
    assert!(svg.contains("a&amp;b"));
}

#[test]
fn render_all_writes_one_file_per_plugin() {
    let snap = obj(&[
        ("cpu", obj(&[("total", Value::Float(10.0))])),
        ("mem", obj(&[("percent", Value::Float(50.0))])),
    ]);
    let fields = flat(&snap);
    let mut map = BTreeMap::new();
    graph::append_fields(&mut map, &fields, 1000.0, 300);
    let files = graph::render_all(&map, &fields, &cfg());
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"cpu.svg"));
    assert!(names.contains(&"mem.svg"));
}

#[test]
fn write_creates_svg_files() {
    let dir = std::env::temp_dir().join("glances-rs-graph-test");
    let _ = std::fs::remove_dir_all(&dir);
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(10.0))]))]);
    let fields = flat(&snap);
    let c = graph::Config {
        path: dir.to_string_lossy().into_owned(),
        width: 200,
        height: 150,
        max_points: 300,
    };
    graph::write(&fields, &c).expect("write");
    let svg = std::fs::read_to_string(dir.join("cpu.svg")).expect("svg file");
    assert!(svg.contains("<svg "));
    let _ = std::fs::remove_dir_all(&dir);
}
