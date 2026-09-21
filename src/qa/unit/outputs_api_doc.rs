//! Unit tests for outputs::api_doc (M12 `--api-doc-restful` printer).

use crate::outputs::api_doc::{render, ENDPOINTS};

#[test]
fn endpoints_collection_is_non_empty() {
    assert!(!ENDPOINTS.is_empty());
    // Upstream Python Glances has ~20 documented endpoints; we should
    // expose at least a dozen to be useful.
    assert!(ENDPOINTS.len() >= 12, "expected ≥12 endpoints, got {}", ENDPOINTS.len());
}

#[test]
fn every_endpoint_is_well_formed() {
    for ep in ENDPOINTS {
        assert!(!ep.method.is_empty(), "endpoint has empty method");
        assert!(!ep.path.is_empty(), "endpoint has empty path");
        assert!(!ep.description.is_empty(), "endpoint {} has empty description", ep.path);
        assert!(ep.path.starts_with('/'), "path must start with /: {}", ep.path);
        // Method must be a standard HTTP verb (uppercase).
        assert!(ep.method.chars().all(|c| c.is_ascii_uppercase()),
                "method must be uppercase: {}", ep.method);
    }
}

#[test]
fn render_contains_core_routes() {
    let s = render();
    // Must list the four "meta" routes users hit first.
    assert!(s.contains("/api/4/status"));
    assert!(s.contains("/api/4/all"));
    assert!(s.contains("/api/4/pluginslist"));
    assert!(s.contains("/api/4/{plugin}"));
    assert!(s.contains("/api/4/processes/{pid}"));
}

#[test]
fn render_contains_method_and_path_columns() {
    let s = render();
    // Both GET and POST should appear (events clear + token are POST).
    assert!(s.contains("GET"));
    assert!(s.contains("POST"));
}

#[test]
fn render_starts_with_banner() {
    let s = render();
    assert!(s.starts_with("Glances REST API documentation"),
            "render() must start with a banner; got: {:?}", &s[..s.len().min(80)]);
}

#[test]
fn render_uses_consistent_version_segment() {
    let s = render();
    assert!(s.contains("/api/4/"));
    // No accidental version drift.
    assert!(!s.contains("/api/3/"));
    assert!(!s.contains("/api/5/"));
    assert!(!s.contains("/api/2/"));
}

#[test]
fn token_endpoint_is_post() {
    let token = ENDPOINTS.iter().find(|e| e.path.contains("/token"));
    assert!(token.is_some(), "missing /token endpoint");
    assert_eq!(token.unwrap().method, "POST");
}

#[test]
fn events_clear_endpoint_is_post() {
    let clear = ENDPOINTS.iter().find(|e| e.path.contains("/events/clear"));
    assert!(clear.is_some(), "missing /events/clear endpoint");
    assert_eq!(clear.unwrap().method, "POST");
}

#[test]
fn openapi_spec_covers_every_endpoint() {
    // The embedded OpenAPI document must document every dumped route
    // and track the crate version (bump the JSON with releases).
    // Full schema validation is manual (paste into editor.swagger.io):
    // the tree has no general JSON parser, so this pins coverage.
    const SPEC: &str = include_str!("../../../assets/static/openapi.json");
    assert!(SPEC.contains("\"openapi\": \"3.1.0\""), "spec must declare OpenAPI 3.1");
    let ver = format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(SPEC.contains(&ver), "spec version must track the crate version");
    for ep in ENDPOINTS {
        let marker = format!("\"{}\"", ep.path);
        assert!(SPEC.contains(&marker), "spec must document {}", ep.path);
    }
}
