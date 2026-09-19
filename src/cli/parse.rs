//! Tokenizer for argv.
//!
//! Splits raw argv strings into typed `Token`s (`Flag`, `WithValue`, `Positional`).
//! The dispatch in `flags.rs` decides what each token does.

/// A single argv token after minimal parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Flag(String),
    WithValue { name: String, value: String },
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
            // --flag=value or -x=value form
            if arg.starts_with("--") || arg.starts_with('-') {
                let (name, value) = arg.split_at(eq_pos);
                out.push(Token::WithValue {
                    name: name.to_string(),
                    value: value[1..].to_string(),
                });
            } else {
                out.push(Token::Positional(arg.clone()));
            }
            i += 1;
            continue;
        }
        if arg.starts_with("--") {
            let name = arg.clone();
            // Long flag may take a value (next argv if non-negative).
            if i + 1 < argv.len() && !argv[i + 1].starts_with('-') && looks_like_value_for(&name) {
                out.push(Token::WithValue { name, value: argv[i + 1].clone() });
                i += 2;
            } else {
                out.push(Token::Flag(name));
                i += 1;
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Short flag(s). Check if THIS specific arg takes a value
            // (only when len == 2, i.e. a single short flag).
            let name = arg.clone();
            if name.len() == 2 && i + 1 < argv.len() && !argv[i + 1].starts_with('-')
                && looks_like_value_for(&name) {
                out.push(Token::WithValue { name, value: argv[i + 1].clone() });
                i += 2;
            } else {
                // Multi-char combo: emit one Flag per char.
                for ch in name.chars().skip(1) {
                    out.push(Token::Flag(format!("-{}", ch)));
                }
                i += 1;
            }
        } else {
            out.push(Token::Positional(arg.clone()));
            i += 1;
        }
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
        | "--export-kafka-bootstrap"
        | "--export-mqtt-server"
        | "--export-statsd-host"
        | "--export-graphite-host"
        | "--export-rabbitmq-url"
        | "--export-mongodb-uri"
        | "--export-cassandra-host"
        | "--export-elasticsearch-host"
        | "--export-opentsdb-host"
        | "--export-riemann-host"
        | "--export-zeromq-endpoint"
        | "--export-nats-server"
        | "--export-couchdb-host"
        | "--export-clickhouse-host"
        | "--export-timescaledb-host"
        | "--export-prometheus-port"
        | "--export-restful-url"
        | "--export-influxdb-host"
        | "--export-influxdb2-org"
        | "--export-influxdb2-bucket"
        | "--export-influxdb2-token"
    )
}
