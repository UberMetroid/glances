//! Server-Sent Events (SSE) framing.
//!
//! Glances' web UI streams live plugin updates over SSE — the browser
//! opens `GET /api/<plugin>/values/stream`, and we push one event per
//! refresh tick. Wire format per the WHATWG HTML spec:
//!
//!   event: <name>\n
//!   data: <json>\n
//!   id: <seq>\n
//!   \n
//!
//! Multiple `data:` lines per event are concatenated with '\n'.

/// Build one SSE event frame. Always ends with the trailing blank line.
pub fn format_event(event: &str, data: &str, id: Option<u64>) -> String {
    let mut out = String::with_capacity(data.len() + event.len() + 32);
    if !event.is_empty() {
        out.push_str("event: ");
        out.push_str(event);
        out.push('\n');
    }
    for line in data.split('\n') {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }
    if let Some(seq) = id {
        out.push_str(&format!("id: {}\n", seq));
    }
    out.push('\n');
    out
}

/// Build the SSE preamble — the HTTP headers sent before any event frame.
/// Caller passes these into a Response builder.
pub fn response_headers() -> Vec<(&'static str, String)> {
    vec![
        ("Content-Type", "text/event-stream".to_string()),
        ("Cache-Control", "no-store".to_string()),
        ("X-Accel-Buffering", "no".to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_basic_event() {
        let f = format_event("stats", r#"{"cpu":1.0}"#, Some(7));
        assert!(f.starts_with("event: stats\n"));
        assert!(f.contains(r#"data: {"cpu":1.0}"#));
        assert!(f.contains("id: 7\n"));
        assert!(f.ends_with("\n\n"));
    }
    #[test]
    fn multi_line_data() {
        let f = format_event("e", "line1\nline2", None);
        assert!(f.contains("data: line1\n"));
        assert!(f.contains("data: line2\n"));
    }
}
