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

fn main() -> ExitCode {
    let args = parse_args();
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
    let _ = effective_refresh;

    match args.mode {
        Mode::Help => { help::print_help(); }
        Mode::Version => { println!("glances-rs {}", env!("CARGO_PKG_VERSION")); }
        Mode::Issue => { print_issue(&config, &pw); }
        Mode::ApiDoc => { print_api_doc(); }
        _ => {
            println!(
                "glances-rs: mode {:?} not yet implemented (later milestone; see PLAN.md)",
                args.mode
            );
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

fn print_api_doc() {
    println!("REST API documentation will be added in milestone M14.");
    println!("See PLAN.md §6.2 AC-16 for the 30+ endpoints to be implemented.");
}
