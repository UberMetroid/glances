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

    // [ip] public_api opt-in — nothing is fetched unless configured
    // (upstream parity: the feature is off without a configured API).
    glances_rs::plugins::ip::configure_public(&config);

    // Startup banner only for long-running modes — printing it for
    // --help/--version/--issue pollutes stdout-adjacent tooling.
    if !matches!(
        args.mode,
        Mode::Help | Mode::Version | Mode::Issue | Mode::ApiDoc | Mode::Fetch | Mode::ModulesList
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
            glances_rs::core::stats::spawn_refresh_loop(stats.clone(), effective_refresh, args.clone());
            logger::info(&format!(
                "web server listening on {}:{} (auth={}, xmlrpc={}, mcp={})",
                args.bind_address, args.web_port, args.auth_enabled,
                args.server_port, args.mcp_path
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
        Mode::XmlRpcServer => {
            run_xmlrpc_server(effective_refresh, &args, &config);
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
            // TTY + not quiet → interactive curses-style UI; otherwise
            // the one-line snapshot loop (pipes, --quiet, --stop-after
            // still behave exactly as before).
            if std::io::IsTerminal::is_terminal(&std::io::stdout()) && !args.quiet {
                let stats = GlancesStats::new(effective_refresh);
                register(&stats, &args, &config);
                if let Err(e) = outputs::tui::run(&stats, &args) {
                    logger::error(&format!("tui: {}", e));
                    return ExitCode::FAILURE;
                }
            } else {
                run_standalone(effective_refresh, &args, &config);
            }
        }
    }
    ExitCode::SUCCESS
}

/// Plugins disabled by light mode (-2..-5 / --light): the optional,
/// higher-cost collectors. Core stats (cpu/mem/load/network/fs/…) stay.
/// `smart`/`vms` spawn subprocesses every tick, so they are light-off.
const LIGHT_DISABLED: &[&str] = &[
    "percpu", "irq", "sensors", "gpu", "npu", "wifi", "raid", "folders",
    "ports", "connections", "containers", "cloud", "amps", "alert", "mpp",
    "smart", "vms",
];

/// Register plugins honoring --enable-plugin, --disable-plugin, and the
/// --light subset. Single entry point so every mode agrees.
fn register(stats: &GlancesStats, args: &glances_rs::cli::args::Args, config: &glances_rs::core::config::Config) {
    // Long-running modes record per-plugin numeric history unless
    // `--disable-history` was passed (upstream default is on).
    stats
        .history_enabled
        .store(!args.disable_history, std::sync::atomic::Ordering::Relaxed);
    let mut disabled: Vec<String> = args.disable_plugins.clone();
    if args.disable_process {
        disabled.extend(
            ["processcount", "processlist", "programlist"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    if args.light {
        disabled.extend(LIGHT_DISABLED.iter().map(|s| s.to_string()));
    }
    glances_rs::plugins::register_filtered(stats, &disabled, &args.enable_plugins);
    // Load `[<plugin>] careful/warning/critical` thresholds into each
    // plugin's limits map — feeds /api/<p>/limits and future alerting.
    stats.apply_limits_config(config);
}

/// `--fetch`: neofetch-style summary printed once and exit (upstream
/// `--stdout-fetch` parity). One refresh tick, then host + load lines.
fn print_fetch(refresh_secs: f32, args: &glances_rs::cli::args::Args, config: &Config) {
    let stats = GlancesStats::new(refresh_secs);
    register(&stats, args, config);
    if let Err(e) = stats.update() {
        logger::warning(&format!("fetch: stats.update() failed: {}", e));
    }
    let snap = stats.snapshot();
    let str_of = |plugin: &str, key: &str| -> String {
        snap.as_object()
            .and_then(|o| o.get(plugin))
            .and_then(|p| p.as_object())
            .and_then(|o| o.get(key))
            .and_then(|v| match v {
                glances_rs::core::value::Value::String(s) => Some(s.clone()),
                _ => v.as_f64().map(|n| format!("{}", n)),
            })
            .unwrap_or_else(|| "-".to_string())
    };
    let num = |plugin: &str, key: &str| -> f64 {
        snap.as_object()
            .and_then(|o| o.get(plugin))
            .and_then(|p| p.as_object())
            .and_then(|o| o.get(key))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    };
    let template = args.fetch_template.clone().unwrap_or_default();
    if !template.is_empty() {
        println!("{}", template);
        return;
    }
    println!("glances-rs {}", env!("CARGO_PKG_VERSION"));
    println!("-------------------");
    println!("Host: {}", str_of("system", "hostname"));
    println!("OS: {} {}", str_of("system", "os_name"), str_of("system", "os_version"));
    println!("Kernel: {}", str_of("system", "kernel"));
    println!("Uptime: {}s", num("uptime", "seconds") as u64);
    println!("CPU: {:.1}% ({} cores)", num("cpu", "total"), num("cpu", "cpucore") as u64);
    println!("Memory: {:.1}%", num("mem", "percent"));
    println!(
        "Load: {:.2} {:.2} {:.2}",
        num("load", "min1"),
        num("load", "min5"),
        num("load", "min15")
    );
    println!("Processes: {}", num("processcount", "total") as u64);
}

/// `--modules-list`: plugin + exporter inventory and exit.
fn print_modules() {
    println!("Plugins:");
    for name in glances_rs::plugins::plugin_names() {
        println!("  {}", name);
    }
    println!("Exporters:");
    for name in glances_rs::exports::exporter_names() {
        println!("  {}", name);
    }
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

/// Minimal standalone monitor until the curses TUI lands: one compact
/// status line per refresh tick (cpu/mem/load), honoring --stop-after
/// and --quiet.
fn run_standalone(refresh_secs: f32, args: &glances_rs::cli::args::Args, config: &Config) {
    use std::io::IsTerminal;
    let stats = GlancesStats::new(refresh_secs);
    register(&stats, args, config);
    // A monitor loop makes no sense without a terminal — emit one
    // snapshot and exit (same exit shape the old stub had), unless the
    // caller explicitly asked for N ticks via --stop-after.
    let one_shot = !std::io::stdout().is_terminal() && args.stop_after.is_none();
    let mut tick: u32 = 0;
    loop {
        if let Err(e) = stats.update() {
            logger::warning(&format!("standalone: stats.update() failed: {}", e));
        }
        if !args.export_targets.is_empty() {
            let keys = stats.plugin_keys();
            glances_rs::exports::write_targets(&stats.snapshot(), args, &keys);
        }
        if !args.quiet {
            let snap = stats.snapshot();
            println!("{}", standalone_line(&snap));
        }
        tick = tick.saturating_add(1);
        if one_shot {
            break;
        }
        if let Some(max) = args.stop_after {
            if tick >= max { break; }
        }
        if refresh_secs > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f32(refresh_secs));
        }
    }
}

/// Compact one-line summary for the standalone loop.
fn standalone_line(snap: &glances_rs::core::value::Value) -> String {
    let get = |plugin: &str, key: &str| -> f64 {
        snap.as_object()
            .and_then(|o| o.get(plugin))
            .and_then(|p| p.as_object())
            .and_then(|o| o.get(key))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    };
    format!(
        "cpu: {:.1}%  mem: {:.1}%  load: {:.2} {:.2} {:.2}  uptime: {}s",
        get("cpu", "total"),
        get("mem", "percent"),
        get("load", "min1"),
        get("load", "min5"),
        get("load", "min15"),
        get("uptime", "seconds") as u64,
    )
}

/// XML-RPC server mode (`-s`): bounded thread-per-connection, HTTP/1.1
/// POST framing compatible with Python `xmlrpc.client`.
fn run_xmlrpc_server(refresh_secs: f32, args: &glances_rs::cli::args::Args, config: &Config) {
    let stats = std::sync::Arc::new(GlancesStats::new(refresh_secs));
    register(&stats, args, config);
    glances_rs::core::stats::spawn_refresh_loop(stats.clone(), refresh_secs, args.clone());
    outputs::xmlrpc_transport::run_server(stats, args);
}

/// XML-RPC client mode (`-c`): single HTTP `getAll` call, print body.
fn run_xmlrpc_client(host: &str, port: u16) {
    outputs::xmlrpc_transport::run_client(host, port);
}
