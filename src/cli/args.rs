//! Command-line model: run modes, SNMP versions, and parsed options.
//!
//! Tokenizing lives in `parse`, per-flag effects in `flags`; this file
//! owns the `Args` struct, its defaults, and the parse entry points.

use super::flags::apply_flag;
use super::parse::parse_argv;

/// Run mode selected by the flags.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Standalone,
    /// `-c HOST`: SNMP polling client (plain `-c` without SNMP flags
    /// errors — the old XML-RPC client no longer exists).
    Client,
    WebServer,
    StdoutCsv,
    StdoutJson,
    StdoutPath,
    /// `--fetch`: one summary screen, then exit.
    Fetch,
    /// `--modules-list`: plugin inventory, then exit.
    ModulesList,
    ApiDoc,
    Issue,
    Help,
    Version,
    /// `--ping ADDR`: probe one health endpoint, then exit.
    Ping,
}

/// SNMP wire version requested on the command line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnmpVersion { V1, V2c, V3 }

/// Every parsed option. Defaults are load-bearing (tests pin them).
#[derive(Debug, Clone)]
pub struct Args {
    pub mode: Mode,
    pub debug: bool,
    pub quiet: bool,
    pub light: bool,
    pub disable_left_sidebar: bool,
    pub disable_quicklook: bool,
    pub full_quicklook: bool,
    pub disable_top: bool,
    pub refresh_time: f32,
    pub config_path: Option<String>,
    pub plugins_dir: Option<String>,
    pub web_port: u16,
    pub bind_address: String,
    pub username: Option<String>,
    pub password: Option<String>,
    /// `-u <name>`: name on the command line (forces a password prompt
    /// on the server side).
    pub username_used: Option<String>,
    /// Bare `--username` / `--password`: prompt on stdin instead.
    pub username_prompt: bool,
    pub password_prompt: bool,
    /// `--fs-free-space`: Free column instead of Used in fs.
    pub fs_free_space: bool,
    pub disable_history: bool,
    pub disable_webui: bool,
    pub disable_config_exec: bool,
    pub disable_plugins: Vec<String>,
    pub enable_plugins: Vec<String>,
    pub stop_after: Option<u32>,
    pub process_filter: Option<String>,
    pub client_host: Option<String>,
    pub url_prefix: String,
    pub stdout_spec: Option<String>,
    pub auth_enabled: bool,
    pub mcp_path: String,
    pub secure_config_path: Option<String>,
    pub snmp_community: Option<String>,
    pub snmp_port: u16,
    pub snmp_version: SnmpVersion,
    pub snmp_user: Option<String>,
    pub snmp_auth: Option<String>,
    pub snmp_force: bool,
    // Display toggles (parsed always; inert without a terminal UI).
    pub disable_bold: bool,
    pub disable_bg: bool,
    pub enable_separator: bool,
    pub disable_cursor: bool,
    pub disable_unicode: bool,
    pub fahrenheit: bool,
    pub sparkline: bool,
    pub byte_units: bool,
    pub percpu: bool,
    pub disable_irix: bool,
    pub mean_gpu: bool,
    pub programs: bool,
    pub arrow_keys_sort: bool,
    pub sort_processes: Option<String>,
    pub process_focus: Option<String>,
    pub process_short_name: bool,
    pub hide_kernel_threads: bool,
    pub diskio_show_ramfs: bool,
    pub diskio_iops: bool,
    pub diskio_latency: bool,
    pub enable_process_extended: bool,
    pub hide_public_info: bool,
    pub strftime_format: String,
    // Modes / outputs.
    pub open_web_browser: bool,
    pub enable_mcp: bool,
    pub fetch_template: Option<String>,
    pub stdout_plugins: Option<String>,
    pub disable_process: bool,
    /// `--access-log`: log each HTTP request (method, path, status).
    pub access_log: bool,
    /// `--ping ADDR`: probe target as `host:port`.
    pub ping_target: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            mode: Mode::Standalone,
            debug: false,
            quiet: false,
            light: false,
            disable_left_sidebar: false,
            disable_quicklook: false,
            full_quicklook: false,
            disable_top: false,
            refresh_time: 2.0,
            config_path: None,
            plugins_dir: None,
            web_port: 61208,
            bind_address: "0.0.0.0".to_string(),
            username: None,
            password: None,
            username_used: None,
            username_prompt: false,
            password_prompt: false,
            fs_free_space: false,
            disable_history: false,
            disable_webui: false,
            disable_config_exec: false,
            disable_plugins: Vec::new(),
            enable_plugins: Vec::new(),
            stop_after: None,
            process_filter: None,
            client_host: None,
            url_prefix: String::new(),
            stdout_spec: None,
            auth_enabled: false,
            mcp_path: "/mcp".to_string(),
            secure_config_path: None,
            snmp_community: None,
            snmp_port: 161,
            snmp_version: SnmpVersion::V2c,
            snmp_user: None,
            snmp_auth: None,
            snmp_force: false,
            disable_bold: false,
            disable_bg: false,
            enable_separator: true,
            disable_cursor: false,
            disable_unicode: false,
            fahrenheit: false,
            sparkline: false,
            byte_units: false,
            percpu: false,
            disable_irix: false,
            mean_gpu: false,
            programs: false,
            arrow_keys_sort: false,
            sort_processes: None,
            process_focus: None,
            process_short_name: true,
            hide_kernel_threads: false,
            diskio_show_ramfs: false,
            diskio_iops: false,
            diskio_latency: false,
            enable_process_extended: false,
            hide_public_info: false,
            strftime_format: String::new(),
            open_web_browser: false,
            enable_mcp: false,
            fetch_template: None,
            stdout_plugins: None,
            disable_process: false,
            access_log: false,
            ping_target: None,
        }
    }
}

/// Parse the process argv (lossy: non-UTF-8 entries never panic).
pub fn parse_args() -> Args {
    let argv: Vec<String> =
        std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    parse_args_with(&argv)
}

/// Parse an explicit argv slice (the test-friendly entry point).
pub fn parse_args_with(argv: &[String]) -> Args {
    let mut args = Args::default();
    for token in &parse_argv(argv) {
        apply_flag(&mut args, token);
    }
    args
}
