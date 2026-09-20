//! Dispatch-layer tests — `--export` targets actually reach exporters.

use std::collections::{BTreeMap, HashMap};

use crate::cli::args::Args;
use crate::core::value::Value;
use crate::exports;
use crate::qa::harness::TempDir;

fn snap() -> Value {
    let mut cpu = BTreeMap::new();
    cpu.insert("total".to_string(), Value::Float(42.5));
    let mut plugins = BTreeMap::new();
    plugins.insert("cpu".to_string(), Value::Object(cpu));
    Value::Object(plugins)
}

fn args_with(target: &str, opts: &[(&str, &str)]) -> Args {
    Args {
        export_targets: vec![target.to_string()],
        export_opts: opts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        ..Args::default()
    }
}

#[test]
fn csv_target_writes_rows_to_configured_path() {
    let dir = TempDir::new("dispatch-csv");
    let path = dir.path().join("out.csv");
    let p = path.to_string_lossy().into_owned();
    let args = args_with("csv", &[("csv-file", p.as_str())]);
    exports::write_targets(&snap(), &args, &HashMap::new());
    let text = std::fs::read_to_string(&path).expect("csv output file must exist");
    assert!(text.contains("cpu"), "csv output missing plugin row: {}", text);
}

#[test]
fn json_target_writes_snapshot_to_configured_path() {
    let dir = TempDir::new("dispatch-json");
    let path = dir.path().join("out.json");
    let p = path.to_string_lossy().into_owned();
    let args = args_with("json", &[("json-file", p.as_str())]);
    exports::write_targets(&snap(), &args, &HashMap::new());
    let text = std::fs::read_to_string(&path).expect("json output file must exist");
    assert!(text.contains("\"cpu\""), "json output missing plugin: {}", text);
}

#[test]
fn unknown_target_is_logged_not_fatal() {
    // Dispatch errors are warnings — a bogus target must not kill the loop.
    let args = args_with("definitely-not-an-exporter", &[]);
    exports::write_targets(&snap(), &args, &HashMap::new()); // must not panic
}

#[test]
fn empty_targets_is_noop() {
    exports::write_targets(&snap(), &Args::default(), &HashMap::new());
}
