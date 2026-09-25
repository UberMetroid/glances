//! Flag effects: apply one token to `Args`.
//!
//! Later flags override earlier ones. Unknown flags are ignored (bare
//! positionals warn); accepted-but-unimplemented flags are explicit
//! no-ops so `--help` output stays truthful.

use super::args::{Args, Mode, SnmpVersion};
use super::parse::Token;

/// Apply one token. Pure dispatch — bare, valued, and positional each
/// have their own table below.
pub fn apply_flag(args: &mut Args, token: &Token) {
    match token {
        Token::Flag(name) => apply_bare(args, name),
        Token::WithValue { name, value } => apply_valued(args, name, value),
        Token::Positional(p) => {
            // Bare words select nothing; warn instead of guessing.
            if !p.is_empty() {
                crate::core::logger::warning(&format!("ignoring unexpected argument: {p}"));
            }
        }
    }
}

fn apply_bare(args: &mut Args, name: &str) {
    match name {
        "-d" | "--debug" => args.debug = true,
        "-q" | "--quiet" => args.quiet = true,
        "-w" | "--webserver" => args.mode = Mode::WebServer,
        "-h" | "--help" => args.mode = Mode::Help,
        "-V" | "--version" => args.mode = Mode::Version,
        "--stdout-csv" => args.mode = Mode::StdoutCsv,
        "--stdout-json" => args.mode = Mode::StdoutJson,
        "--fetch" | "--stdout-fetch" => args.mode = Mode::Fetch,
        "--modules-list" | "--module-list" => args.mode = Mode::ModulesList,
        "--api-doc" | "--api-restful-doc" | "--api-doc-restful" => args.mode = Mode::ApiDoc,
        "--issue" => args.mode = Mode::Issue,
        "--disable-history" => args.disable_history = true,
        "--disable-webui" => args.disable_webui = true,
        "--disable-config-exec" => args.disable_config_exec = true,
        "--disable-check-update" | "--memory-leak" | "--trace-malloc" | "--disable-plugin-warn" => {}
        "--fs-free-space" => args.fs_free_space = true,
        "--disable-process" => args.disable_process = true,
        "--snmp-force" => args.snmp_force = true,
        "--open-web-browser" => args.open_web_browser = true,
        "--enable-mcp" => args.enable_mcp = true,
        "--enable-irq" => enable_one(args, "irq"),
        "--auth-enabled" => args.auth_enabled = true,
        "--access-log" => args.access_log = true,
        "--username" => args.username_prompt = true,
        "--password" => args.password_prompt = true,
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
        "-2" | "--disable-left-sidebar" => args.disable_left_sidebar = true,
        "-3" | "--disable-quicklook" => args.disable_quicklook = true,
        "-4" | "--full-quicklook" => args.full_quicklook = true,
        "-5" | "--disable-top" => args.disable_top = true,
        "--light" | "--enable-light" => args.light = true,
        _ => {}
    }
}

fn apply_valued(args: &mut Args, name: &str, value: &str) {
    match name {
        "-t" | "--time" => {
            // NaN/inf/non-positive values would break every refresh
            // comparison — only finite positives land.
            if let Ok(v) = value.parse::<f32>()
                && v.is_finite() && v > 0.0 {
                    args.refresh_time = v;
                }
        }
        "--stdout" => {
            args.mode = Mode::StdoutPath;
            args.stdout_spec = Some(value.to_string());
        }
        "--stdout-csv" | "--stdout-json" => {
            // Bare form selects the mode; the valued form also records
            // which plugins to stream.
            args.mode = if name == "--stdout-csv" { Mode::StdoutCsv } else { Mode::StdoutJson };
            args.stdout_plugins = Some(value.to_string());
        }
        "--web-port" => {
            if let Ok(v) = value.parse::<u16>() {
                args.web_port = v;
            }
        }
        "--snmp-port" => {
            if let Ok(v) = value.parse::<u16>() {
                args.snmp_port = v;
            }
        }
        "--stop-after" => {
            if let Ok(v) = value.parse::<u32>() {
                args.stop_after = Some(v);
            }
        }
        "--disable-plugin" => extend_list(&mut args.disable_plugins, value),
        "--enable-plugin" => extend_list(&mut args.enable_plugins, value),
        "-C" | "--config" => args.config_path = Some(value.to_string()),
        "-f" | "--process-filter" => args.process_filter = Some(value.to_string()),
        "-P" | "--plugins" => args.plugins_dir = Some(value.to_string()),
        "-c" | "--client" => {
            args.client_host = Some(value.to_string());
            args.mode = Mode::Client;
        }
        "-B" | "--bind" => args.bind_address = value.to_string(),
        // `-u` takes the name; secrets never ride argv (process lists
        // leak) — `--password` always prompts instead.
        "-u" => args.username_used = Some(value.to_string()),
        "--sort-processes" => args.sort_processes = Some(value.to_string()),
        "--process-focus" => args.process_focus = Some(value.to_string()),
        "--strftime" => args.strftime_format = value.to_string(),
        "--fetch-template" | "--stdout-fetch-template" => {
            args.fetch_template = Some(value.to_string());
        }
        "--snmp-community" => args.snmp_community = Some(value.to_string()),
        "--snmp-user" => args.snmp_user = Some(value.to_string()),
        "--snmp-auth" => args.snmp_auth = Some(value.to_string()),
        "--snmp-version" => {
            args.snmp_version = match value {
                "1" => SnmpVersion::V1,
                "2c" => SnmpVersion::V2c,
                "3" => SnmpVersion::V3,
                _ => SnmpVersion::V2c,
            };
        }
        "--url-prefix" => args.url_prefix = value.to_string(),
        "--mcp-path" => args.mcp_path = value.to_string(),
        "--secure-config" => args.secure_config_path = Some(value.to_string()),
        "--ping" => {
            args.ping_target = Some(value.to_string());
            args.mode = Mode::Ping;
        }
        _ => {}
    }
}

/// Append one plugin name unless already enabled.
fn enable_one(args: &mut Args, plugin: &str) {
    if !args.enable_plugins.iter().any(|e| e == plugin) {
        args.enable_plugins.push(plugin.to_string());
    }
}

/// Extend a plugin list from a comma-separated value (trimmed, blanks
/// dropped).
fn extend_list(into: &mut Vec<String>, value: &str) {
    into.extend(value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
}
