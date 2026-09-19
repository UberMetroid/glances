//! Flag dispatch — applies a single `Token` to the `Args` struct.
//!
//! Each known flag is matched here. Unknown flags are logged and ignored.
//! Full flag set will be completed in M2; this M0 stub covers the most
//! common cases so the binary can respond to `--help` and `--version`.

use super::args::{Args, Mode};
use super::parse::Token;

/// Apply a single parsed token to `args`. Idempotent: later flags override
/// earlier ones (matching Python Glances behavior).
pub fn apply_flag(args: &mut Args, token: &Token) {
    match token {
        Token::Flag(name) => match name.as_str() {
            "-d" | "--debug" => args.debug = true,
            "-s" | "--server" => args.mode = Mode::XmlRpcServer,
            "-w" | "--webserver" => args.mode = Mode::WebServer,
            "--browser" => args.mode = Mode::Browser,
            "-q" | "--quiet" => args.quiet = true,
            "-h" | "--help" => args.mode = Mode::Help,
            "-V" | "--version" => args.mode = Mode::Version,
            "--disable-history" => args.disable_history = true,
            "--disable-webui" => args.disable_webui = true,
            "--disable-config-exec" => args.disable_config_exec = true,
            "--disable-check-update" => { /* no-op in M0; full impl in M2 */ }
            "--api-doc-restful" => args.mode = Mode::ApiDoc,
            "--issue" => args.mode = Mode::Issue,
            _ => {
                // Unknown flag — ignore silently for M0. M2 will add a warning.
            }
        },
        Token::WithValue { name, value } => match name.as_str() {
            "-t" | "--time" => {
                if let Ok(v) = value.parse::<f32>() {
                    args.refresh_time = v;
                }
            }
            "-C" | "--config" => args.config_path = Some(value.clone()),
            "-P" | "--plugins" => args.plugins_dir = Some(value.clone()),
            "-c" | "--client" => {
                args.client_host = Some(value.clone());
                args.mode = Mode::XmlRpcClient;
            }
            "-p" | "--port" => {
                if let Ok(v) = value.parse::<u16>() {
                    args.server_port = v;
                }
            }
            "-B" | "--bind" => args.bind_address = value.clone(),
            "-u" | "--username" => args.username = Some(value.clone()),
            "--password" => args.password = Some(value.clone()),
            "--export" => args.export_targets.push(value.clone()),
            "--stop-after" => {
                if let Ok(v) = value.parse::<u32>() {
                    args.stop_after = Some(v);
                }
            }
            "--url-prefix" => args.url_prefix = value.clone(),
            "--process-filter" => args.process_filter = Some(value.clone()),
            _ => { /* unknown with-value flag ignored in M0 */ }
        },
        Token::Positional(p) => {
            // `--stdout <spec>` is a positional argument after `--stdout`.
            // In M0 we only recognize it as a hint; full handling in M12.
            if !p.is_empty() && !p.starts_with('-') {
                args.mode = Mode::StdoutPath;
            }
        }
    }
}
