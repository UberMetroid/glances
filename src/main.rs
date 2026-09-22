//! glances-rs binary entry point.
//!
//! M2: parses CLI args, loads config + password file, dispatches to mode.
//! Subsequent milestones replace the body with real mode dispatch.

use std::process::ExitCode;

use glances_rs::cli::args::{parse_args, Mode};
use glances_rs::cli::help;
use glances_rs::cli::modes::{print_fetch, print_modules, register};
use glances_rs::core::config::Config;
use glances_rs::core::config_dir;
use glances_rs::core::logger;
use glances_rs::core::password::PasswordFile;
use glances_rs::core::stats::GlancesStats;
use glances_rs::outputs;
use glances_rs::outputs::web;

fn main() -> ExitCode {
    let mut args = parse_args();
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
    let mut pw = match PasswordFile::load(&pw_path) {
        Ok(p) => p,
        Err(e) => {
            logger::warning(&format!("could not load password file at {:?}: {}", pw_path, e));
            PasswordFile::empty()
        }
    };

    // Server/client login/password (upstream `main.py:810-843`). Only
    // touches stdin when a prompt flag was passed.
    glances_rs::core::password::resolve_mode_auth(&mut args, &mut pw);

    // [ip] public_api opt-in — nothing is fetched unless configured
    // (upstream parity: the feature is off without a configured API).
    glances_rs::plugins::ip::configure_public(&config);

    // Startup banner only for long-running modes — printing it for
    // --help/--version/--issue pollutes stdout-adjacent tooling.
    if !matches!(
        args.mode,
        Mode::Help | Mode::Version | Mode::Issue | Mode::ApiDoc | Mode::Fetch | Mode::ModulesList
        | Mode::Ping
    ) {
        logger::info(&format!(
            "glances-rs {} starting (mode={:?}, refresh={}s, plugins_dir={:?}, config={:?}, password_file={:?})",
            env!("CARGO_PKG_VERSION"),
            args.mode,
            args.refresh_time,
            args.plugins_dir,
            config_path,
            pw_path,
        ));
    }

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
        Mode::Fetch => { print_fetch(effective_refresh, &args, &config); }
        Mode::ModulesList => { print_modules(); }
        Mode::StdoutCsv => { run_stdout_csv(effective_refresh, &args, &config); }
        Mode::StdoutJson => { run_stdout_json(effective_refresh, &args, &config); }
        Mode::StdoutPath => {
            let stats = GlancesStats::new(effective_refresh);
            register(&stats, &args, &config);
            let spec = args.stdout_spec.clone().unwrap_or_default();
            outputs::stdout_path::run(&stats, &spec, effective_refresh, args.stop_after);
        }
        Mode::WebServer => {
            let stats = std::sync::Arc::new(GlancesStats::new(effective_refresh));
            register(&stats, &args, &config);
            // The web server has no update driver of its own — spawn the
            // shared refresh loop so plugins actually tick (previously
            // every endpoint served permanently-stale empty stats).
            glances_rs::core::idle::spawn_refresh_loop(stats.clone(), effective_refresh);
            logger::info(&format!(
                "web server listening on {}:{} (auth={}, mcp={})",
                args.bind_address, args.web_port, args.auth_enabled, args.mcp_path
            ));
            if args.open_web_browser {
                // Give the listener a beat to bind, then open the UI.
                let url = format!("http://{}:{}/", args.bind_address, args.web_port);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    let _ = std::process::Command::new("xdg-open").arg(&url).output();
                });
            }
            if let Err(e) = web::run(stats, &args, Some(pw)) {
                logger::error(&format!("web server stopped: {}", e));
                return ExitCode::FAILURE;
            }
        }
        Mode::Client => {
            if !args.snmp_force {
                eprintln!("glances-rs: plain client mode died with XML-RPC; use -w plus the REST API, or -c <host> with --snmp-* for SNMP");
                return ExitCode::FAILURE;
            }
            match args.client_host.clone() {
                Some(host) => {
                    if !glances_rs::cli::snmp_mode::run_snmp_client(
                        &host, effective_refresh, &args, &config,
                    ) {
                        return ExitCode::FAILURE;
                    }
                }
                None => {
                    eprintln!("glances-rs: -c/--client requires a host");
                    return ExitCode::FAILURE;
                }
            }
        }
        Mode::Standalone => {
            eprintln!("glances-rs: the terminal UI was removed; use -w for the dashboard + REST API");
            return ExitCode::FAILURE;
        }
        Mode::Ping => {
            match args.ping_target.clone() {
                Some(t) if glances_rs::cli::ping::ping_once(&t) => {}
                Some(t) => {
                    eprintln!("glances-rs: health probe failed for {t}");
                    return ExitCode::FAILURE;
                }
                None => {
                    eprintln!("glances-rs: --ping requires an address (host:port)");
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    ExitCode::SUCCESS
}

/// Display subsets live in `cli::modes::register` (one upstream meaning
/// per flag: -2 sidebar, -3 quicklook, -4 full-quicklook, -5 top menu,
/// --light the manage-light set). Nothing is disabled here.
fn print_issue(_config: &Config, _pw: &PasswordFile) {
    println!("glances-rs {} debug/system info dump", env!("CARGO_PKG_VERSION"));
    println!("OS: {}", std::env::consts::OS);
    println!("Arch: {}", std::env::consts::ARCH);
    println!("Family: {}", std::env::consts::FAMILY);
    println!("Rustc version: see cargo --version");
    println!("Binary path: see which glances-rs");
    println!("(more fields land in M12)");
}

fn run_stdout_csv(refresh_secs: f32, args: &glances_rs::cli::args::Args, config: &Config) {
    let stats = GlancesStats::new(refresh_secs);
    register(&stats, args, config);
    outputs::csv_stdout::run(&stats, refresh_secs, args.stop_after, args);
}

fn run_stdout_json(refresh_secs: f32, args: &glances_rs::cli::args::Args, config: &Config) {
    let stats = GlancesStats::new(refresh_secs);
    register(&stats, args, config);
    outputs::json_stdout::run(&stats, refresh_secs, args.stop_after, args);
}
