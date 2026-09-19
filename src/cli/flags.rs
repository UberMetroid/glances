//! Flag dispatch — applies a single `Token` to the `Args` struct.
//!
//! Each known flag is matched here. Unknown flags are logged and ignored.
//! Full flag set per plan §4.4. Implemented incrementally; future milestones
//! add behavior to each flag's setter.

use super::args::{Args, Mode, SnmpVersion};
use super::parse::Token;

/// Apply a single parsed token to `args`. Idempotent: later flags override
/// earlier ones (matching Python Glances behavior).
pub fn apply_flag(args: &mut Args, token: &Token) {
    match token {
        Token::Flag(name) => match name.as_str() {
            // Mode selection.
            "-d" | "--debug" => args.debug = true,
            "-s" | "--server" => args.mode = Mode::XmlRpcServer,
            "-w" | "--webserver" => args.mode = Mode::WebServer,
            "--browser" => args.mode = Mode::Browser,
            "-q" | "--quiet" => args.quiet = true,
            "-h" | "--help" => args.mode = Mode::Help,
            "-V" | "--version" => args.mode = Mode::Version,
            // Toggles.
            "--disable-history" => args.disable_history = true,
            "--disable-webui" => args.disable_webui = true,
            "--disable-config-exec" => args.disable_config_exec = true,
            "--disable-check-update" => { /* M12: check PyPI/GitHub */ }
            "--api-doc-restful" => args.mode = Mode::ApiDoc,
            "--issue" => args.mode = Mode::Issue,
            "--memory-leak" => { /* M12 */ }
            "--trace-malloc" => { /* M12 */ }
            "--disable-plugin-warn" => { /* M12 */ }
            "--fs-free-space" => { /* M12 */ }
            // Light mode toggles (-2 / -3 / -4 / -5 / --light).
            "-2" => args.light = true,
            "-3" => args.light = true,
            "-4" => args.light = true,
            "-5" => args.light = true,
            "--light" => args.light = true,
            // Auth.
            "--auth-enabled" => args.auth_enabled = true,
            _ => { /* unknown flag — log + ignore */ }
        },
        Token::WithValue { name, value } => match name.as_str() {
            "-t" | "--time" => { if let Ok(v) = value.parse::<f32>() { args.refresh_time = v; } }
            "-C" | "--config" => args.config_path = Some(value.clone()),
            "-P" | "--plugins" => args.plugins_dir = Some(value.clone()),
            "-c" | "--client" => { args.client_host = Some(value.clone()); args.mode = Mode::XmlRpcClient; }
            "-p" | "--port" => { if let Ok(v) = value.parse::<u16>() { args.server_port = v; } }
            "-B" | "--bind" => args.bind_address = value.clone(),
            "-u" | "--username" => args.username = Some(value.clone()),
            "--password" => args.password = Some(value.clone()),
            "--export" => args.export_targets.push(value.clone()),
            "--export-csv-file" => args.export_files.push(value.clone()),
            "--export-json-file" => args.export_files.push(value.clone()),
            "--export-influxdb-file" => args.export_files.push(value.clone()),
            "--export-influxdb2-file" => args.export_files.push(value.clone()),
            "--export-influxdb3-file" => args.export_files.push(value.clone()),
            "--export-prometheus-file" => args.export_files.push(value.clone()),
            "--process-filter" => args.process_filter = Some(value.clone()),
            "--stop-after" => { if let Ok(v) = value.parse::<u32>() { args.stop_after = Some(v); } }
            "--url-prefix" => args.url_prefix = value.clone(),
            "--cached-time" => { if let Ok(v) = value.parse::<u32>() { args.cached_time = v; } }
            "--snmp-community" => args.snmp_community = Some(value.clone()),
            "--snmp-port" => { if let Ok(v) = value.parse::<u16>() { args.snmp_port = v; } }
            "--snmp-version" => {
                args.snmp_version = match value.as_str() {
                    "1" => SnmpVersion::V1,
                    "2c" => SnmpVersion::V2c,
                    "3" => SnmpVersion::V3,
                    _ => SnmpVersion::V2c,
                };
            }
            "--mcp-path" => args.mcp_path = value.clone(),
            "--secure-config" => args.secure_config_path = Some(value.clone()),
            _ => { /* unknown with-value flag */ }
        },
        Token::Positional(p) => {
            // `--stdout <spec>` is a positional after `--stdout`.
            if !p.is_empty() && !p.starts_with('-') {
                args.mode = Mode::StdoutPath;
                args.stdout_spec = Some(p.clone());
            }
        }
    }
}
