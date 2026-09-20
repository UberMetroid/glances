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
            "--stdout-csv" => args.mode = Mode::StdoutCsv,
            "--stdout-json" => args.mode = Mode::StdoutJson,
            "--fetch" | "--stdout-fetch" => args.mode = Mode::Fetch,
            "--modules-list" | "--module-list" => args.mode = Mode::ModulesList,
            "--api-doc" | "--api-restful-doc" => args.mode = Mode::ApiDoc,
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
            "--export-csv-overwrite" => args.export_csv_overwrite = true,
            "--disable-process" => args.disable_process = true,
            "--disable-autodiscover" => args.disable_autodiscover = true,
            "--snmp-force" => args.snmp_force = true,
            "--open-web-browser" => args.open_web_browser = true,
            "--enable-mcp" => args.enable_mcp = true,
            "--enable-irq" => {
                if !args.enable_plugins.iter().any(|e| e == "irq") {
                    args.enable_plugins.push("irq".to_string());
                }
            }
            // Display toggles (upstream parity; consumed by the TUI).
            "--disable-bold" => args.disable_bold = true,
            "--disable-bg" => args.disable_bg = true,
            "--disable-separator" => args.enable_separator = false,
            "--disable-cursor" => args.disable_cursor = true,
            "--disable-unicode" => args.disable_unicode = true,
            "--fahrenheit" => args.fahrenheit = true,
            "--sparkline" => args.sparkline = true,
            "-b" | "--byte" => args.byte_units = true,
            "-1" | "--percpu" | "--per-cpu" => args.percpu = true,
            "-0" | "--disable-irix" => args.disable_irix = true,
            "-6" | "--meangpu" => args.mean_gpu = true,
            "--programs" | "--program" => args.programs = true,
            "--arrow-keys-sort" => args.arrow_keys_sort = true,
            "--process-long-name" => args.process_short_name = false,
            "--process-short-name" => args.process_short_name = true,
            "--hide-kernel-threads" => args.hide_kernel_threads = true,
            "--diskio-show-ramfs" => args.diskio_show_ramfs = true,
            "--diskio-iops" => args.diskio_iops = true,
            "--diskio-latency" => args.diskio_latency = true,
            "--enable-process-extended" => args.enable_process_extended = true,
            "--hide-public-info" => args.hide_public_info = true,
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
            "-t" | "--time" => {
                // "nan"/"inf" parse as f32 but break every refresh
                // comparison — only accept finite positive values.
                if let Ok(v) = value.parse::<f32>() {
                    if v.is_finite() && v > 0.0 { args.refresh_time = v; }
                }
            }
            "--stdout" => { args.mode = Mode::StdoutPath; args.stdout_spec = Some(value.clone()); }
            "--web-port" => { if let Ok(v) = value.parse::<u16>() { args.web_port = v; } }
            "--disable-plugin" => {
                args.disable_plugins.extend(value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
            }
            "--enable-plugin" => {
                args.enable_plugins.extend(value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
            }
            "-C" | "--config" => args.config_path = Some(value.clone()),
            "-f" | "--process-filter" => args.process_filter = Some(value.clone()),
            "-P" | "--plugins" => args.plugins_dir = Some(value.clone()),
            "-c" | "--client" => { args.client_host = Some(value.clone()); args.mode = Mode::XmlRpcClient; }
            "-p" | "--port" => { if let Ok(v) = value.parse::<u16>() { args.server_port = v; } }
            "-B" | "--bind" => args.bind_address = value.clone(),
            "-u" | "--username" => args.username = Some(value.clone()),
            "--password" => args.password = Some(value.clone()),
            // Upstream accepts a comma-separated list.
            "--export" => {
                args.export_targets.extend(
                    value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                );
            }
            "--sort-processes" => args.sort_processes = Some(value.clone()),
            "--process-focus" => args.process_focus = Some(value.clone()),
            "--strftime" => args.strftime_format = value.clone(),
            "--fetch-template" | "--stdout-fetch-template" => {
                args.fetch_template = Some(value.clone());
            }
            "--export-process-filter" => args.export_process_filter = Some(value.clone()),
            "--snmp-auth" => args.snmp_auth = Some(value.clone()),
            "--snmp-user" => args.snmp_user = Some(value.clone()),
            "--stdout-csv" | "--stdout-json" => {
                // Bare `--stdout-csv` selects the mode; with a value it
                // also records the plugin list (upstream parity).
                args.mode = if name == "--stdout-csv" {
                    Mode::StdoutCsv
                } else {
                    Mode::StdoutJson
                };
                args.stdout_plugins = Some(value.clone());
            }
            "--export-csv-file" | "--export-json-file" | "--export-influxdb-file"
            | "--export-influxdb2-file" | "--export-influxdb3-file" | "--export-prometheus-file" => {
                args.export_files.push(value.clone());
                args.export_opts.push((name["--export-".len()..].to_string(), value.clone()));
            }
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
            _ => {
                // Generic `--export-<opt> <value>` capture: every other
                // advertised exporter option (mqtt-server, kafka-bootstrap,
                // riemann-host, ...) is recorded for the dispatch layer
                // without needing a dedicated arm per flag.
                if let Some(opt) = name.strip_prefix("--export-") {
                    args.export_opts.push((opt.to_string(), value.clone()));
                }
            }
        },
        Token::Positional(p) => {
            // Glances takes no positional args — previously any bare
            // positional (or the orphaned value of an unhandled flag)
            // silently flipped the mode to StdoutPath. Warn instead.
            if !p.is_empty() {
                crate::core::logger::warning(&format!("ignoring unexpected argument: {}", p));
            }
        }
    }
}
