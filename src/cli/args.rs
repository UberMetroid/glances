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
        }
    }
}

/// Parse `std::env::args()` and return the resolved `Args`.
pub fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().skip(1).collect();
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
