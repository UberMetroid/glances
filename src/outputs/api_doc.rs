//! M12 — REST API documentation printer (`--api-doc-restful`).
//!
//! Prints a static list of REST endpoints that M14 will implement. The
//! list mirrors `glances/outputs/glances_stdout_api_restful_doc.py`:
//! each entry is `METHOD path` followed by a one-line description.
//!
//! This module is intentionally a plain-text dump: M14 will deliver
//! the live server. For now `--api-doc-restful` shows users what to
//! expect so they can prepare downstream tooling.

/// One entry in the API doc: an HTTP method + path + one-line summary.
pub struct Endpoint {
    pub method: &'static str,
    pub path: &'static str,
    pub description: &'static str,
}

/// Canonical REST API endpoint list. Mirrors the Python Glances
/// `GlancesStdoutApiRestfulDoc` so users get the same surface to plan
/// against. `__apiversion__` is rendered as `4` (the current
/// `__apiversion__` in upstream Glances).
pub const ENDPOINTS: &[Endpoint] = &[
    Endpoint { method: "GET",  path: "/api/4/status",
               description: "API liveness check (returns 200 + Glances version)." },
    Endpoint { method: "GET",  path: "/api/4/pluginslist",
               description: "List all registered plugin names." },
    Endpoint { method: "GET",  path: "/api/4/all",
               description: "All stats for every plugin in one big dictionary." },
    Endpoint { method: "GET",  path: "/api/4/all/limits",
               description: "Threshold / limit dictionary for every plugin." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}",
               description: "Stats for a single plugin (e.g. /api/4/cpu)." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/limits",
               description: "Threshold / limit dictionary for a single plugin." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/{item}",
               description: "Single field value from a plugin's stats object." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/{item}/description",
               description: "Human-readable description of a stat field." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/{item}/unit",
               description: "Unit string for a stat field (percent, bytes, etc.)." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/{item}/value/{value}",
               description: "Item whose `key` field matches the given value." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/history",
               description: "Per-field history list for a plugin." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/history/{n}",
               description: "Last `n` values of the plugin history." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/{item}/history",
               description: "History for a single field of a plugin." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/{item}/history/{n}",
               description: "Last `n` values of a single field's history." },
    Endpoint { method: "GET",  path: "/api/4/{plugin}/top/{n}",
               description: "Top `n` items of a list-style plugin (e.g. processlist)." },
    Endpoint { method: "GET",  path: "/api/4/processes/{pid}",
               description: "Stats for a single process by PID." },
    Endpoint { method: "POST", path: "/api/4/processes/extended/{pid}",
               description: "Enable extended stats for a single process (one at a time)." },
    Endpoint { method: "POST", path: "/api/4/events/clear/all",
               description: "Clear all alerts from the events list." },
    Endpoint { method: "POST", path: "/api/4/events/clear/{severity}",
               description: "Clear alerts of a given severity (warning/critical)." },
    Endpoint { method: "POST", path: "/api/4/token",
               description: "Exchange username/password for a JWT bearer token." },
    Endpoint { method: "GET",  path: "/docs",
               description: "Embedded Swagger / OpenAPI documentation UI." },
];

/// Render the full documentation to a string. Stable for unit testing.
pub fn render() -> String {
    let mut buf = String::new();
    buf.push_str("Glances REST API documentation (M14 preview)\n");
    buf.push_str("============================================\n\n");
    buf.push_str("The endpoints below are exposed by --webserver once M14 lands.\n");
    buf.push_str("Path placeholders: {plugin}, {item}, {value}, {pid}, {n}, {severity}.\n\n");
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
