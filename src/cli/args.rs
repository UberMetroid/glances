//! Args struct + Mode enum + parse_args entry point.
//!
//! Full parser lives in `parse.rs`; the public surface here is what
//! `main.rs` and other modules consume.

use super::parse::parse_argv;
use super::flags::apply_flag;

/// Top-level CLI mode selected by flag dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Standalone,
    XmlRpcServer,
    XmlRpcClient,
    Browser,
    WebServer,
    StdoutCsv,
    StdoutJson,
    StdoutPath,
    /// `--fetch`: print a neofetch-style summary and exit.
    Fetch,
    /// `--modules-list`: print plugins + exporters and exit.
    ModulesList,
    ApiDoc,
    Issue,
    Help,
    Version,
}

/// SNMP protocol version (per `glances/main.py:444-453`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnmpVersion { V1, V2c, V3 }

/// Parsed CLI arguments, populated by `parse_args`.
#[derive(Debug, Clone)]
pub struct Args {
    pub mode: Mode,
    pub debug: bool,
    pub quiet: bool,
    pub light: bool,
    pub refresh_time: f32,
    pub cached_time: u32,
    pub config_path: Option<String>,
    pub plugins_dir: Option<String>,
    pub server_port: u16,
    pub web_port: u16,
    pub bind_address: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub disable_history: bool,
    pub disable_webui: bool,
    pub disable_config_exec: bool,
    pub disable_plugins: Vec<String>,
    pub enable_plugins: Vec<String>,
    pub export_targets: Vec<String>,
    pub export_files: Vec<String>,
    /// Values of `--export-<name>-*` flags (e.g. `mqtt-server`,
    /// `csv-file`), stored without the `--export-` prefix so the
    /// exporter dispatch can look up per-exporter options generically.
    pub export_opts: Vec<(String, String)>,
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
    pub disable_autodiscover: bool,
    // Display toggles (upstream `main.py` parity; consumed by the TUI).
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
    // Export options.
    pub export_csv_overwrite: bool,
    pub export_process_filter: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            mode: Mode::Standalone,
            debug: false,
            quiet: false,
            light: false,
            refresh_time: 2.0,
            cached_time: 1,
            config_path: None,
            plugins_dir: None,
            server_port: 61209,
            web_port: 61208,
            bind_address: "0.0.0.0".to_string(),
            username: None,
            password: None,
            disable_history: false,
            disable_webui: false,
            disable_config_exec: false,
            disable_plugins: Vec::new(),
            enable_plugins: Vec::new(),
            export_targets: Vec::new(),
            export_files: Vec::new(),
            export_opts: Vec::new(),
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
            disable_autodiscover: false,
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
            export_csv_overwrite: false,
            export_process_filter: None,
        }
    }
}

/// Parse `std::env::args()` and return the resolved `Args`.
pub fn parse_args() -> Args {
    // args_os + lossy conversion: std::env::args() panics on non-UTF-8
    // argv entries, which would crash the binary before flag handling.
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    parse_args_with(&argv)
}

/// Parse an explicit argv list and return the resolved `Args`. Test-friendly
/// variant of `parse_args()` that doesn't read the environment.
pub fn parse_args_with(argv: &[String]) -> Args {
    let mut args = Args::default();
    let tokens = parse_argv(argv);
    for token in &tokens {
        apply_flag(&mut args, token);
    }
    args
}
