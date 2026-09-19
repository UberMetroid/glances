//! Tests for the containers plugin — JSON parsing, Docker projection,
//! missing-socket behaviour, and registry wiring.

use std::collections::BTreeMap;

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins::containers::{
    self, collect, parse_json_array, project, ContainersPlugin, NAME,
};

// Minimal Docker Engine API /containers/json fixture — one container
// with one TCP port mapping. Fields beyond what we project are included
// to confirm the parser tolerates them.
const DOCKER_JSON_FIXTURE: &str = r#"[
  {
    "Id": "d3a1b2c3e4f5061728394a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
    "Created": 1700000000,
    "Path": "/bin/sh",
    "Args": ["-c", "echo hi"],
    "State": "running",
    "Status": "Up 5 minutes",
    "Image": "nginx:latest",
    "Names": ["/my_container", "/my_container/aliases"],
    "Ports": [
      {"PrivatePort": 80, "PublicPort": 8080, "Type": "tcp", "IP": "0.0.0.0"}
    ],
    "HostConfig": {"NetworkMode": "default"}
  },
  {
    "Id": "abandoned",
    "Names": ["/stopped_one"],
    "Image": "alpine:3",
    "State": "exited",
    "Status": "Exited (0) 1 minute ago",
    "Created": 1699999000
  }
]"#;

#[test]
fn parse_json_array_handles_docker_fixture() {
    let arr = parse_json_array(DOCKER_JSON_FIXTURE).expect("parse must succeed");
    assert_eq!(arr.len(), 2);
    let first = arr[0].as_object().expect("first element is an object");
    assert_eq!(first.get("State").and_then(Value::as_str), Some("running"));
    // Names is a JSON array — the parser preserves it as Value::Array.
    let names = first.get("Names").and_then(Value::as_array).expect("names array");
    assert_eq!(names.len(), 2);
    // Ports is an array of nested objects with numeric fields.
    let ports = first.get("Ports").and_then(Value::as_array).expect("ports array");
    assert_eq!(ports.len(), 1);
    let p0 = ports[0].as_object().expect("port entry");
    assert_eq!(p0.get("PrivatePort").and_then(Value::as_i64), Some(80));
    assert_eq!(p0.get("PublicPort").and_then(Value::as_i64), Some(8080));
}

#[test]
fn project_strips_leading_slash_from_first_name() {
    let arr = parse_json_array(DOCKER_JSON_FIXTURE).unwrap();
    let first_obj = arr[0].as_object().unwrap().clone();
    let row = project(&first_obj);
    let row_obj = row.as_object().expect("projected row is an object");
    // Name field is the first element of `Names` with the leading "/" stripped.
    assert_eq!(row_obj.get("name").and_then(Value::as_str), Some("my_container"));
    assert_eq!(row_obj.get("engine").and_then(Value::as_str), Some("docker"));
    assert_eq!(row_obj.get("image").and_then(Value::as_str), Some("nginx:latest"));
    assert_eq!(row_obj.get("state").and_then(Value::as_str), Some("running"));
    assert_eq!(row_obj.get("status").and_then(Value::as_str), Some("Up 5 minutes"));
    assert_eq!(row_obj.get("created").and_then(Value::as_i64), Some(1700000000));
    assert!(row_obj.get("id").and_then(Value::as_str).unwrap().starts_with("d3a"));
    // Ports is preserved verbatim.
    let ports = row_obj.get("ports").and_then(Value::as_array).expect("ports");
    assert_eq!(ports.len(), 1);
}

#[test]
fn collect_returns_empty_when_socket_missing() {
    // /var/run/docker.sock may or may not be present on the test host.
    // Either way, `collect` must return a Vec — never panic — and
    // when the socket is absent it must be empty.
    let v = collect("/var/run/this_socket_must_not_exist_for_test_xyz_9999");
    assert!(v.is_empty(), "missing socket must yield empty Vec");
}

#[test]
fn plugin_update_produces_empty_array_when_no_docker() {
    let mut p = ContainersPlugin::new();
    p.update().expect("update must not error even without docker");
    let arr = p.stats().as_array().expect("stats must be an array");
    // Whether the test host has docker or not, the update is non-fatal.
    // If docker is present, `arr.len()` would be > 0; we only assert
    // shape, not contents.
    for v in arr {
        let obj = v.as_object().expect("each row is an object");
        assert!(obj.contains_key("id"));
        assert!(obj.contains_key("name"));
        assert!(obj.contains_key("engine"));
        assert!(obj.contains_key("image"));
        assert!(obj.contains_key("state"));
        assert!(obj.contains_key("status"));
        assert!(obj.contains_key("created"));
        assert!(obj.contains_key("ports"));
    }
}

#[test]
fn register_plugin_appears_in_stats() {
    let s = GlancesStats::new(1.0);
    containers::register(&s);
    assert!(s.plugin_names().contains(&NAME));
}

#[test]
fn get_key_returns_id() {
    let p = ContainersPlugin::new();
    assert_eq!(p.get_key(), Some("id"));
}

#[test]
fn parse_json_array_rejects_garbage() {
    assert!(parse_json_array("not json").is_none());
    assert!(parse_json_array("").is_none());
    // Not an array — must be rejected, not silently coerced.
    assert!(parse_json_array("{\"a\":1}").is_none());
}

#[test]
fn project_handles_missing_optional_fields() {
    // Minimal object — only Id present. All other fields fall back to ""/0/Null.
    let mut obj: BTreeMap<String, Value> = BTreeMap::new();
    obj.insert("Id".into(), Value::String("xyz".into()));
    let row = project(&obj);
    let m = row.as_object().unwrap();
    assert_eq!(m.get("id").and_then(Value::as_str), Some("xyz"));
    assert_eq!(m.get("name").and_then(Value::as_str), Some(""));
    assert_eq!(m.get("created").and_then(Value::as_i64), Some(0));
    assert_eq!(m.get("ports"), Some(&Value::Null));
}
