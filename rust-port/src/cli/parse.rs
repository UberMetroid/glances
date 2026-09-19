//! Tokenizer for argv.
//!
//! Splits raw argv strings into typed `Token`s (`Flag`, `WithValue`, `Positional`).
//! The dispatch in `flags.rs` decides what each token does. Keeping the
//! tokenizer separate from the dispatcher means each file stays under the
//! 256-line cap.

/// A single argv token after minimal parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A bare `--flag` with no value attached.
    Flag(String),
    /// A `--flag value` pair (or `--flag=value`).
    WithValue { name: String, value: String },
    /// A positional argument (no leading dash).
    Positional(String),
}

/// Tokenize a flat list of argv strings. No globbing, no expansion — strictly
/// shell-quoting-free positional split.
pub fn parse_argv(argv: &[String]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let arg = &argv[i];
        if let Some(eq_pos) = arg.find('=') {
            // --flag=value form
            if arg.starts_with("--") || (arg.starts_with('-') && !arg.starts_with("--") && eq_pos == 2) {
                let (name, value) = arg.split_at(eq_pos);
                out.push(Token::WithValue {
                    name: name.to_string(),
                    value: value[1..].to_string(),
                });
            } else {
                out.push(Token::Positional(arg.clone()));
            }
        } else if arg.starts_with("--") || (arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--")) {
            let name = arg.clone();
            // Short flags may combine (e.g. -qw); for now treat each as a flag.
            if name.starts_with("--") {
                // Check if next argv is a value (doesn't start with -).
                if i + 1 < argv.len() && !argv[i + 1].starts_with('-') && looks_like_value_for(&name) {
                    out.push(Token::WithValue { name, value: argv[i + 1].clone() });
                    i += 1;
                } else {
                    out.push(Token::Flag(name));
                }
            } else {
                // Short flag(s) — emit one Flag per char.
                for ch in name.chars().skip(1) {
                    out.push(Token::Flag(format!("-{}", ch)));
                }
            }
        } else {
            out.push(Token::Positional(arg.clone()));
        }
        i += 1;
    }
    out
}

/// Flags that always take a following value.
fn looks_like_value_for(flag: &str) -> bool {
    matches!(flag,
        "-t" | "--time"
        | "-c" | "--client"
        | "-p" | "--port"
        | "-B" | "--bind"
        | "-u" | "--username"
        | "-C" | "--config"
        | "-P" | "--plugins"
        | "--export"
        | "--export-csv-file"
        | "--export-json-file"
        | "--export-influxdb-file"
        | "--export-influxdb2-file"
        | "--export-influxdb3-file"
        | "--export-prometheus-file"
        | "--process-filter"
        | "--stop-after"
        | "--url-prefix"
        | "--cached-time"
        | "--snmp-community"
        | "--snmp-port"
        | "--snmp-version"
        | "--password"
        | "--mcp-path"
        | "--secure-config"
    )
}
