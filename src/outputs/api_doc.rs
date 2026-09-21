//! REST API documentation printer (`--api-doc-restful`).
//!
//! Prints the endpoint list the `-w` server actually serves: each
//! entry is `METHOD path` followed by a one-line description.
//! `docs/api.md` is the full reference with examples; a router test
//! pins every listed route so the two cannot drift apart.

/// One entry in the API doc: an HTTP method + path + one-line summary.
pub struct Endpoint {
    pub method: &'static str,
    pub path: &'static str,
    pub description: &'static str,
}

/// Served REST endpoints. Every entry must route to a handler (a
/// router test pins this); unserved upstream shapes (`/{plugin}/{item}`,
/// `/top/{n}`, per-plugin `/limits`, `/docs`) are documented in
/// `docs/api.md` as 404s instead of being listed here.
pub const ENDPOINTS: &[Endpoint] = &[
    Endpoint { method: "GET",  path: "/api/4/status",
               description: "Liveness check: {\"version\"} of this build." },
    Endpoint { method: "GET",  path: "/api/4/pluginslist",
               description: "Registered plugin names in registration order." },
    Endpoint { method: "GET",  path: "/api/4/serverslist",
               description: "Always []: no server list on a standalone server." },
    Endpoint { method: "GET",  path: "/api/4/all",
               description: "Every plugin's current object in one dictionary." },
    Endpoint { method: "GET",  path: "/api/all/values",
               description: "Alias of /api/4/all (what the dashboard polls)." },
    Endpoint { method: "GET",  path: "/api/all/stats",
               description: "Alias of /api/4/all." },
    Endpoint { method: "GET",  path: "/api/4/all/limits",
               description: "Alert thresholds per plugin." },
    Endpoint { method: "GET",  path: "/api/all/limits",
               description: "Alias of /api/4/all/limits." },
    Endpoint { method: "GET",  path: "/api/4/all/views",
               description: "Per-plugin view metadata (sort key, fields, decorations)." },
    Endpoint { method: "GET",  path: "/api/all/views",
               description: "Alias of /api/4/all/views." },
    Endpoint { method: "GET",  path: "/api/all/description",
               description: "Alias of /api/4/all/views." },
    Endpoint { method: "GET",  path: "/api/4/history",
               description: "Recorded history for every plugin." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}",
               description: "One plugin's current object (e.g. /api/4/cpu)." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/values",
               description: "Same payload, explicit whole-plugin form." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/description",
               description: "Field catalog: [{\"name\", \"unit\"}] for the plugin." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/history",
               description: "Per-field [[timestamp, value]] arrays for a plugin." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/history/{n}",
               description: "Tail of the plugin history (0 = all)." },
    Endpoint { method: "GET",  path: "/api/4/processes/{pid}",
               description: "One process by pid (404 when absent)." },
    Endpoint { method: "GET",  path: "/api/4/processes/extended",
               description: "Pinned extended-stats process, or {} when none." },
    Endpoint { method: "POST", path: "/api/4/processes/extended/{pid}",
               description: "Pin extended stats for one process (one at a time)." },
    Endpoint { method: "POST", path: "/api/4/processes/extended/disable",
               description: "Unpin the extended-stats process." },
    Endpoint { method: "POST", path: "/api/4/events/clear/all",
               description: "Drop all alerts." },
    Endpoint { method: "POST", path: "/api/4/events/clear/warning",
               description: "Drop warning alerts (no critical-only route)." },
    Endpoint { method: "GET",  path: "/api/4/events/stream",
               description: "Server-Sent Events; opens with a hello event." },
    Endpoint { method: "POST", path: "/api/4/token",
               description: "501: no JWT issuer in pure-std builds (use Basic auth)." },
    Endpoint { method: "GET",  path: "/healthz",
               description: "Ops check: plain-text ok (not JSON)." },
    Endpoint { method: "GET",  path: "/openapi.json",
               description: "This API as an OpenAPI 3.1 document." },
];

/// Render the full documentation to a string. Stable for unit testing.
pub fn render() -> String {
    let mut buf = String::new();
    buf.push_str("Glances REST API documentation\n");
    buf.push_str("================================\n\n");
    buf.push_str("Served by `glances-rs -w` (default port 61208). Unversioned\n");
    buf.push_str("/api/ aliases serve the same payloads. Full reference with\n");
    buf.push_str("examples: docs/api.md. Placeholders: {plugin}, {pid}, {n}.\n\n");
    for ep in ENDPOINTS {
        // Pad method to 6 chars for column alignment.
        buf.push_str(&format!("{:<6} {}\n    {}\n\n", ep.method, ep.path, ep.description));
    }
    buf
}

/// Print the REST API documentation to stdout.
pub fn print_doc() {
    print!("{}", render());
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_endpoint_has_method_and_path() {
        for ep in ENDPOINTS {
            assert!(!ep.method.is_empty(), "empty method");
            assert!(!ep.path.is_empty(), "empty path");
            assert!(!ep.description.is_empty(), "empty description for {}", ep.path);
            assert!(ep.path.starts_with('/'), "path should start with /: {}", ep.path);
        }
    }

    #[test]
    fn endpoints_include_key_routes() {
        let paths: Vec<&str> = ENDPOINTS.iter().map(|e| e.path).collect();
        assert!(paths.iter().any(|p| p.contains("/status")));
        assert!(paths.iter().any(|p| p.contains("/all")));
        assert!(paths.iter().any(|p| p.contains("/pluginslist")));
        assert!(paths.iter().any(|p| p.contains("/processes/{pid}")));
        assert!(paths.iter().any(|p| p.contains("/token")));
    }

    #[test]
    fn render_starts_with_header_and_lists_endpoints() {
        let s = render();
        assert!(s.contains("Glances REST API documentation"));
        assert!(s.contains("/api/4/status"));
        assert!(s.contains("/api/4/all"));
    }

    #[test]
    fn render_uses_consistent_version_segment() {
        let s = render();
        // Every /api/... path should use the same version segment ("/4/").
        assert!(s.contains("/api/4/"));
        // No other version number should sneak in.
        assert!(!s.contains("/api/3/"));
        assert!(!s.contains("/api/5/"));
    }
}
