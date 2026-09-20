//! MCP (Model Context Protocol) over HTTP — minimal std-only surface that
//! mirrors the JSON-RPC 2.0 framing MCP uses.
//!
//! Three methods are recognized:
//!   * `initialize`     → returns server identity + capability list.
//!   * `tools/list`     → returns the (single) `glances.snapshot` tool.
//!   * `tools/call`     → invokes `glances.snapshot` with no args and
//!                          returns the live plugin snapshot as JSON.
//!
//! Anything else returns a JSON-RPC `-32601` (Method not found).
//!
//! Wire format note: we deliberately use plain JSON over HTTP. The full
//! MCP spec is JSON-RPC 2.0, supports streaming, batching, and notifications;
//! this subset covers what the reference `mcp` Python client needs to
//! discover and call `glances.snapshot` against a Rust daemon.

use crate::core::stats::GlancesStats;
use crate::core::value::{self, Value};

/// MCP/JSON-RPC error codes (subset of the spec).
const ERR_METHOD_NOT_FOUND: i32 = -32601;
const ERR_INVALID_PARAMS: i32 = -32602;

const SERVER_INFO: &str = concat!(
    r#"{"name":"glances-rs","version":""#,
    env!("CARGO_PKG_VERSION"),
    r#"","protocol":"mcp/1.0"}"#,
);

const CAPABILITIES: &str = r#"{"tools":{"listChanged":false}}"#;

const TOOLS_LIST: &str = r#"{"tools":[{"name":"glances.snapshot","description":"Return the live Glances plugin snapshot as a JSON object.","inputSchema":{"type":"object","properties":{},"required":[]}}]}"#;

/// Build a JSON-RPC 2.0 success response for the supplied `id` and result.
fn success(id: &Value, result: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}",
        value::to_json(id),
        result
    )
}

/// Build a JSON-RPC 2.0 error response for the supplied `id`, code, and message.
fn error(id: &Value, code: i32, message: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":{},\"message\":\"{}\"}}}}",
        value::to_json(id),
        code,
        message.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// Tiny ad-hoc JSON parser sufficient for the small MCP messages we accept.
/// We don't depend on `serde_json` (AC-1), so we accept the extra few lines
/// of parsing here in exchange for staying crate-free.
fn parse_message(body: &str) -> Option<(String, Value)> {
    let method = extract_string_field(body, "method")?;
    let id = extract_value_field(body, "id").unwrap_or(Value::Null);
    Some((method, id))
}

fn extract_string_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let idx = body.find(&needle)?;
    let after = &body[idx + needle.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    if !rest.starts_with('"') { return None; }
    let rest = &rest[1..];
    let mut out = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other => { out.push('\\'); out.push(other); }
            }
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

fn extract_value_field(body: &str, key: &str) -> Option<Value> {
    let needle = format!("\"{}\"", key);
    let idx = body.find(&needle)?;
    let after = &body[idx + needle.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    let bytes = rest.as_bytes();
    if bytes.is_empty() { return None; }
    match bytes[0] {
        b'n' => Some(Value::Null),
        b't' => Some(Value::Bool(true)),
        b'f' => Some(Value::Bool(false)),
        b'"' => extract_string_field(rest, key).map(Value::String),
        b'0'..=b'9' | b'-' => {
            let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-').unwrap_or(rest.len());
            let n = rest[..end].parse::<f64>().ok()?;
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                Some(Value::Int(n as i64))
            } else {
                Some(Value::Float(n))
            }
        }
        _ => None,
    }
}

/// Build the response body for one MCP/JSON-RPC request.
pub fn handle(body: &str, stats: &GlancesStats) -> String {
    let (method, id) = match parse_message(body) {
        Some(pair) => pair,
        None => return error(&Value::Null, ERR_INVALID_PARAMS, "could not parse method/id"),
    };
    match method.as_str() {
        "initialize" => success(&id, SERVER_INFO),
        "capabilities" | "server/capabilities" => success(&id, CAPABILITIES),
        "tools/list" => success(&id, TOOLS_LIST),
        "tools/call" => {
            // We only accept a single tool name: "glances.snapshot".
            let name = extract_string_field(body, "name").unwrap_or_default();
            if name != "glances.snapshot" {
                return error(&id, ERR_METHOD_NOT_FOUND, &format!("unknown tool: {}", name));
            }
            let mut map = std::collections::BTreeMap::new();
            let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
            for p in guard.iter() {
                map.insert(p.name().to_string(), p.stats().clone());
            }
            // MCP TextContent.text is a JSON *string* — the tool result
            // serialized to text — not a nested object. We also carry the
            // structured result alongside for clients that support it.
            let text = value::to_json(&Value::Object(map.clone()));
            let payload = format!(
                "{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"structuredContent\":{}}}",
                value::to_json(&Value::String(text)),
                value::to_json(&Value::Object(map)),
            );
            success(&id, &payload)
        }
        _ => error(&id, ERR_METHOD_NOT_FOUND, &format!("unknown method: {}", method)),
    }
}

/// Static info about this MCP surface, exposed for the about page.
pub const SURFACE_NAME: &str = "mcp";
pub const SURFACE_VERSION: &str = "1.0";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::stats::GlancesStats;

    #[test]
    fn initialize_returns_server_info() {
        let stats = GlancesStats::new(2.0);
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let r = handle(body, &stats);
        assert!(r.contains("\"result\":{"));
        assert!(r.contains("glances-rs"));
    }

    #[test]
    fn tools_list_returns_single_tool() {
        let stats = GlancesStats::new(2.0);
        let body = r#"{"jsonrpc":"2.0","id":"abc","method":"tools/list"}"#;
        let r = handle(body, &stats);
        assert!(r.contains("glances.snapshot"));
    }

    #[test]
    fn unknown_method_returns_minus_32601() {
        let stats = GlancesStats::new(2.0);
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"foo"}"#;
        let r = handle(body, &stats);
        assert!(r.contains("-32601"));
    }

    #[test]
    fn tools_call_snapshot_returns_plugin_payload() {
        let stats = GlancesStats::new(2.0);
        crate::plugins::register_all(&stats);
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"glances.snapshot"}}"#;
        let r = handle(body, &stats);
        assert!(r.contains("\"content\":"));
        assert!(r.contains("cpu"));
    }

    #[test]
    fn tools_call_text_is_json_string() {
        // MCP spec: TextContent.text is a string, not a nested object.
        let stats = GlancesStats::new(2.0);
        crate::plugins::register_all(&stats);
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"glances.snapshot"}}"#;
        let r = handle(body, &stats);
        assert!(r.contains("\"text\":\""), "text must be a JSON string: {}", r);
        assert!(r.contains("\"type\":\"text\""));
    }
}
