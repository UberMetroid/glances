//! XML-RPC surface — minimal std-only subset that Python Glances clients
//! understand.
//!
//! Only three methods are exposed:
//!   * `getAll()` → returns the full plugin snapshot, the same shape as
//!     `GET /api/all/values`.
//!   * `getPlugin(name)` → returns one plugin's stats, the same shape as
//!     `GET /api/<name>/values`.
//!   * `getAllPlugins()` → returns the list of registered plugin names.
//!
//! The wire format is the standard XML-RPC `<methodResponse>` envelope,
//! with `<value>` cells containing either `<string>`, `<int>`, `<double>`,
//! `<boolean>`, `<array>`, or `<struct>`. We intentionally don't try to
//! be the canonical XML-RPC implementation: just enough for the
//! `xmlrpc.client` server-proxy pattern in Python Glances.

use crate::core::stats::GlancesStats;
use crate::core::value::{self, Value};

/// Render the `<value>` cell for one `Value`. NaN/Infinity become `<string>`.
fn render_value(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("<value><nil/></value>"),
        Value::Bool(b) => out.push_str(&format!("<value><boolean>{}</boolean></value>", if *b { 1 } else { 0 })),
        Value::Int(i) => out.push_str(&format!("<value><int>{}</int></value>", i)),
        Value::Uint(u) => out.push_str(&format!("<value><int>{}</int></value>", u)),
        Value::Float(f) if f.is_nan() || f.is_infinite() => {
            out.push_str(&format!("<value><string>{}</string></value>", f));
        }
        Value::Float(f) => out.push_str(&format!("<value><double>{}</double></value>", f)),
        Value::String(s) => out.push_str(&format!("<value><string>{}</string></value>", xml_escape(s))),
        Value::Array(arr) => {
            out.push_str("<value><array><data>");
            for item in arr { render_value(item, out); }
            out.push_str("</data></array></value>");
        }
        Value::Object(obj) => {
            out.push_str("<value><struct>");
            for (k, val) in obj {
                out.push_str(&format!(
                    "<member><name>{}</name>{}</member>",
                    xml_escape(k),
                    {
                        let mut buf = String::new();
                        render_value(val, &mut buf);
                        buf
                    }
                ));
            }
            out.push_str("</struct></value>");
        }
    }
}

/// Minimal XML special-character escaping for the five XML predefined entities.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Build an XML-RPC `<methodResponse>` for a single return value.
/// `payload` must already be a complete `<value>...</value>` element
/// (as produced by `render_value`) — wrapping it in another `<value>`
/// produces invalid XML-RPC.
fn method_response(payload: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<methodResponse><params><param>{}</param></params></methodResponse>\n",
        payload
    )
}

/// Build an XML-RPC `<methodResponse>` carrying a `<fault>`.
fn method_fault(code: i32, msg: &str) -> String {
    let mut struct_buf = String::new();
    {
        let mut m = std::collections::BTreeMap::new();
        m.insert("faultCode".to_string(), Value::Int(code as i64));
        m.insert("faultString".to_string(), Value::String(msg.to_string()));
        let v = Value::Object(m);
        render_value(&v, &mut struct_buf);
    }
    format!(
        "<?xml version=\"1.0\"?>\n<methodResponse><fault>{}</fault></methodResponse>\n",
        struct_buf
    )
}

/// Build an XML-RPC `<methodResponse>` from a typed return value.
pub fn response_for_value(v: &Value) -> String {
    let mut buf = String::new();
    render_value(v, &mut buf);
    method_response(&buf)
}

/// Build the XML-RPC envelope for a `<fault>`.
pub fn response_for_fault(code: i32, msg: &str) -> String {
    method_fault(code, msg)
}

/// Parse a single `<string>` cell from a method-call body. Returns the
/// argument string if the call has exactly one `<string>` argument.
fn extract_string_arg(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let open = lower.find("<string>")?;
    let close = lower[open..].find("</string>")?;
    let inner = &body[open + "<string>".len()..open + close];
    Some(inner.to_string())
}

/// Dispatch one XML-RPC method call against the live stats container.
pub fn dispatch(method: &str, body: &str, stats: &GlancesStats) -> String {
    match method {
        "getAll" => {
            let mut map = std::collections::BTreeMap::new();
            let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
            for p in guard.iter() {
                map.insert(p.name().to_string(), p.stats().clone());
            }
            response_for_value(&Value::Object(map))
        }
        "getAllPlugins" => {
            let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
            let names: Vec<Value> = guard.iter().map(|p| Value::String(p.name().to_string())).collect();
            response_for_value(&Value::Array(names))
        }
        "getPlugin" => {
            let Some(name) = extract_string_arg(body) else {
                return response_for_fault(1, "getPlugin requires a single string argument");
            };
            let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
            match guard.iter().find(|p| p.name() == name) {
                Some(p) => response_for_value(p.stats()),
                None => response_for_fault(2, &format!("unknown plugin: {}", name)),
            }
        }
        _ => response_for_fault(3, &format!("unknown method: {}", method)),
    }
}

/// Render the full XML-RPC response body for a method `name` and the
/// supplied HTTP request body. Convenience used by `outputs::web`.
pub fn handle(body: &str, stats: &GlancesStats) -> Vec<u8> {
    let method = match extract_method_name(body) {
        Some(m) => m,
        None => return method_fault(4, "missing <methodName>").into_bytes(),
    };
    dispatch(&method, body, stats).into_bytes()
}

/// Pull the `<methodName>` text out of an XML-RPC request body.
pub fn extract_method_name(body: &str) -> Option<String> {
    let open = body.find("<methodName>")?;
    let close = body[open..].find("</methodName>")?;
    Some(body[open + "<methodName>".len()..open + close].to_string())
}

/// Tiny helper used by web plumbing to fetch a snapshot as a string.
pub fn snapshot_string(stats: &GlancesStats) -> String {
    let mut map = std::collections::BTreeMap::new();
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    for p in guard.iter() {
        map.insert(p.name().to_string(), p.stats().clone());
    }
    value::to_json(&Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::stats::GlancesStats;

    #[test]
    fn fault_response_is_well_formed() {
        let s = response_for_fault(7, "boom");
        assert!(s.contains("<fault>"));
        assert!(s.contains("<name>faultCode</name>"));
        assert!(s.contains("<name>faultString</name>"));
        assert!(s.contains("<string>boom</string>"));
    }

    #[test]
    fn extract_method_name_basic() {
        let body = "<?xml version=\"1.0\"?><methodCall><methodName>getAll</methodName><params/></methodCall>";
        assert_eq!(extract_method_name(body).as_deref(), Some("getAll"));
    }

    #[test]
    fn extract_string_arg_basic() {
        let body = "<params><param><value><string>cpu</string></value></param></params>";
        assert_eq!(extract_string_arg(body).as_deref(), Some("cpu"));
    }

    #[test]
    fn dispatch_unknown_method_returns_fault() {
        let stats = GlancesStats::new(2.0);
        let r = dispatch("nope", "", &stats);
        assert!(r.contains("<fault>"));
    }

    #[test]
    fn dispatch_get_all_plugins_returns_array_of_strings() {
        let stats = GlancesStats::new(2.0);
        crate::plugins::register_all(&stats);
        let r = dispatch("getAllPlugins", "", &stats);
        assert!(r.contains("<array><data>"));
        assert!(r.contains("<string>cpu</string>"));
    }

    #[test]
    fn success_response_wraps_value_exactly_once() {
        // Regression: <param> must contain exactly one <value> element —
        // render_value already emits the wrapper.
        let r = response_for_value(&Value::Int(42));
        assert!(!r.contains("<value><value>"), "double-wrapped: {}", r);
        assert!(r.contains("<param><value><int>42</int></value></param>"));
        let opens = r.matches("<value>").count();
        let closes = r.matches("</value>").count();
        assert_eq!(opens, closes);
        assert_eq!(opens, 1);
    }

    #[test]
    fn nested_values_still_balanced() {
        let mut m = std::collections::BTreeMap::new();
        m.insert("a".to_string(), Value::Array(vec![Value::Int(1)]));
        let r = response_for_value(&Value::Object(m));
        assert_eq!(r.matches("<value>").count(), r.matches("</value>").count());
        assert!(!r.contains("<value><value>"));
    }

    #[test]
    fn xml_escapes_five_entities() {
        assert_eq!(xml_escape("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
    }
}
