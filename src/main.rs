//! glances-rs binary entry point.
//!
//! M2: parses CLI args, loads config + password file, dispatches to mode.
//! Subsequent milestones replace the body with real mode dispatch.

use std::process::ExitCode;

use glances_rs::cli::args::{parse_args, Mode};
use glances_rs::cli::help;
use glances_rs::core::config::Config;
use glances_rs::core::config_dir;
use glances_rs::core::logger;
use glances_rs::core::password::PasswordFile;
use glances_rs::core::stats::GlancesStats;
use glances_rs::outputs;
use glances_rs::outputs::web;

fn main() -> ExitCode {
    let args = parse_args();
    glances_rs::platform::assert_linux_host();
    logger::init(args.debug);

    // Resolve config + password paths.
    let config_path = config_dir::resolve(args.config_path.as_deref());
    let config = match Config::from_file(&config_path) {
        Ok(c) => c,
        Err(e) => {
            logger::warning(&format!("could not read config at {:?}: {}; using defaults", config_path, e));
            Config::empty()
        }
    };
    let pw_path = args.secure_config_path.clone()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(PasswordFile::default_path);
    let pw = match PasswordFile::load(&pw_path) {
        Ok(p) => p,
        Err(e) => {
            logger::warning(&format!("could not load password file at {:?}: {}", pw_path, e));
            PasswordFile::empty()
        }
    };

    logger::info(&format!(
        "glances-rs {} starting (mode={:?}, refresh={}s, plugins_dir={:?}, config={:?}, password_file={:?})",
        env!("CARGO_PKG_VERSION"),
        args.mode,
        args.refresh_time,
        args.plugins_dir,
        config_path,
        pw_path,
    ));

    // CLI overrides config (matches `model.py:717-728` semantics).
    // If config has [global]/refresh and CLI -t wasn't passed, use config.
    let effective_refresh = if let Some(v) = config.get_float("global", "refresh") {
        if (args.refresh_time - 2.0_f32).abs() < f32::EPSILON {
            v as f32
        } else {
            args.refresh_time
        }
    } else {
        args.refresh_time
    };

    match args.mode {
        Mode::Help => { help::print_help(); }
        Mode::Version => { println!("glances-rs {}", env!("CARGO_PKG_VERSION")); }
        Mode::Issue => { print_issue(&config, &pw); }
        Mode::ApiDoc => { outputs::api_doc::print_doc(); }
        Mode::StdoutCsv => { run_stdout_csv(effective_refresh, args.stop_after); }
        Mode::StdoutJson => { run_stdout_json(effective_refresh, args.stop_after); }
        Mode::StdoutPath => {
            println!("--stdout <spec> not yet implemented (M12 followup)");
        }
        Mode::WebServer => {
            // Build the plugin container. The web server is the first mode
            // that needs live stats; other modes either print and exit or
            // (for Standalone/TUI) wire their own refresh loop.
            let stats = std::sync::Arc::new(GlancesStats::new(effective_refresh));
            glances_rs::plugins::register_all(&stats);
            logger::info(&format!(
                "web server listening on {}:{} (auth={}, xmlrpc={}, mcp={})",
                args.bind_address, args.web_port, args.auth_enabled,
                args.server_port, args.mcp_path
            ));
            if let Err(e) = web::run(stats, &args, Some(pw)) {
                logger::error(&format!("web server stopped: {}", e));
                return ExitCode::FAILURE;
            }
        }
        Mode::XmlRpcServer => {
            run_xmlrpc_server(effective_refresh, &args);
        }
        Mode::XmlRpcClient => {
            if let Some(host) = args.client_host.clone() {
                run_xmlrpc_client(&host, args.server_port);
            } else {
                println!("glances-rs: --client requires --server <host> (XML-RPC client mode)");
            }
        }
        Mode::Browser => {
            println!("glances-rs: browser mode not yet implemented (M15 followup)");
        }
        Mode::Standalone => {
            // M15a: plain-stdout TUI. The standalone loop owns its
            // plugin container (no Arc needed — single-threaded).
            // tui scaffolding not yet built — temporary stub.
            println!("glances-rs: standalone TUI scaffold pending (M15a)");
        }
    }
    ExitCode::SUCCESS
}

fn print_issue(_config: &Config, _pw: &PasswordFile) {
    println!("glances-rs {} debug/system info dump", env!("CARGO_PKG_VERSION"));
    println!("OS: {}", std::env::consts::OS);
    println!("Arch: {}", std::env::consts::ARCH);
    println!("Family: {}", std::env::consts::FAMILY);
    println!("Rustc version: see cargo --version");
    println!("Binary path: see which glances-rs");
    println!("(more fields land in M12)");
}

fn run_stdout_csv(refresh_secs: f32, stop_after: Option<u32>) {
    let stats = GlancesStats::new(refresh_secs);
    glances_rs::plugins::register_all(&stats);
    outputs::csv_stdout::run(&stats, refresh_secs, stop_after);
}

fn run_stdout_json(refresh_secs: f32, stop_after: Option<u32>) {
    let stats = GlancesStats::new(refresh_secs);
    glances_rs::plugins::register_all(&stats);
    outputs::json_stdout::run(&stats, refresh_secs, stop_after);
}

/// Minimal std-only XML-RPC server. Spawns a single thread per accepted
/// connection, reads the request, dispatches via `outputs::xmlrpc`, and
/// writes the response back. Mirrors Python Glances' `-s` mode.
fn run_xmlrpc_server(refresh_secs: f32, args: &glances_rs::cli::args::Args) {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    let stats = Arc::new(GlancesStats::new(refresh_secs));
    glances_rs::plugins::register_all(&stats);
    let bind = (args.bind_address.clone(), args.server_port);
    let listener = match TcpListener::bind(bind.clone()) {
        Ok(l) => l,
        Err(e) => {
            logger::error(&format!("xmlrpc bind {:?} failed: {}", bind, e));
            return;
        }
    };
    logger::info(&format!("xmlrpc server listening on {}:{}", bind.0, bind.1));
    for stream in listener.incoming() {
        let mut s = match stream {
            Ok(s) => s,
            Err(e) => { logger::warning(&format!("accept failed: {}", e)); continue; }
        };
        let mut buf = [0u8; 8192];
        let n = match s.read(&mut buf) { Ok(n) => n, Err(_) => continue };
        let body = String::from_utf8_lossy(&buf[..n]).into_owned();
        let method = outputs::xmlrpc::extract_method_name(&body).unwrap_or_default();
        let response = outputs::xmlrpc::handle(&body, &stats);
        logger::info(&format!("xmlrpc {} -> {} bytes", method, response.len()));
        if let Err(e) = s.write_all(&response) {
            logger::warning(&format!("xmlrpc write failed: {}", e));
        }
    }
}

/// Minimal XML-RPC client: connect, issue a single `getAll` call, print the
/// pretty-printed XML response. Mirrors Python Glances' `-c` mode.
fn run_xmlrpc_client(host: &str, port: u16) {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let body = b"<?xml version=\"1.0\"?><methodCall><methodName>getAll</methodName><params/></methodCall>";
    let mut s = match TcpStream::connect((host, port)) {
        Ok(s) => s,
        Err(e) => { println!("connect failed: {}", e); return; }
    };
    if let Err(e) = s.write_all(body) { println!("write failed: {}", e); return; }
    let mut buf = Vec::new();
    if let Err(e) = s.read_to_end(&mut buf) { println!("read failed: {}", e); return; }
    let response = String::from_utf8_lossy(&buf);
    println!("{}", response);
}
