//! Startup login/password prompts (upstream `main.py:810-843`).
//!
//! * Server side (`-w`): `--username` prompts for a new name (or
//!   `-u` supplies it, default `glances`); `--password` or `-u` prompts
//!   for a new password *with confirmation* and stores it hashed in
//!   the password file.
//! * Client side (`-c`): prompts only fill `args` for the session; the
//!   password is kept in clear in memory like upstream (`clear=True`).
//!
//! No prompt flag → defaults (`glances`/empty), no stdin touched, so
//! pipes and `--stop-after` runs never block.

use super::PasswordFile;
use crate::cli::args::{Args, Mode};

/// Mode dispatch for `resolve_auth`: servers define credentials,
/// the client only enters them (upstream `main.py:828-840`).
pub fn resolve_mode_auth(args: &mut Args, pw: &mut PasswordFile) {
    match args.mode {
        Mode::WebServer => resolve_auth(args, pw, true),
        Mode::Client => resolve_auth(args, pw, false),
        _ => {}
    }
}

pub fn resolve_auth(args: &mut Args, pw: &mut PasswordFile, server_side: bool) {
    if !args.username_prompt && !args.password_prompt && args.username_used.is_none() {
        args.username = Some("glances".to_string());
        args.password = Some(String::new());
        return;
    }
    // Every username needs a password (upstream `main.py:812-813`).
    if args.username_prompt {
        args.password_prompt = true;
    }
    let username = if args.username_prompt {
        let desc = if server_side { "Enter new username: " } else { "Enter username: " };
        read_line(desc)
    } else if let Some(u) = args.username_used.clone() {
        u
    } else {
        "glances".to_string()
    };
    args.username = Some(username.clone());
    if args.password_prompt || args.username_used.is_some() {
        if server_side {
            loop {
                let first = read_password("Enter new password: ");
                let second = read_password("Confirm password: ");
                if first == second {
                    pw.set(&username, &first);
                    if let Err(e) = pw.save() {
                        crate::core::logger::warning(&format!(
                            "could not save password file: {}",
                            e
                        ));
                    }
                    break;
                }
                crate::core::logger::warning("passwords do not match, try again");
            }
            args.password = None;
        } else {
            args.password = Some(read_password("Enter password: "));
        }
    } else {
        args.password = Some(String::new());
    }
}

/// Visible prompt on stderr, one line from stdin (EOF → empty).
fn read_line(desc: &str) -> String {
    use std::io::Write as _;
    let _ = write!(std::io::stderr(), "{}", desc);
    let _ = std::io::stderr().flush();
    let mut buf = String::new();
    match std::io::stdin().read_line(&mut buf) {
        Ok(_) => buf.trim_end_matches(&['\r', '\n'][..]).to_string(),
        Err(_) => String::new(),
    }
}

/// Password prompt with echo disabled, getpass-style. Echo is toggled
/// via `stty` (argv-only spawn, restored immediately after the read);
/// when that fails (no TTY, missing binary) it falls back to a
/// visible read rather than failing the startup.
fn read_password(desc: &str) -> String {
    use std::io::Write as _;
    let hidden = set_echo(false);
    let pw = read_line(desc);
    if hidden {
        set_echo(true);
    }
    let _ = writeln!(std::io::stderr());
    pw.trim_end_matches(&['\r', '\n'][..]).to_string()
}

/// Toggle TTY echo via `stty`. Returns whether echo was disabled, so
/// the caller only restores what it changed.
fn set_echo(on: bool) -> bool {
    use std::io::Write as _;
    let arg = if on { "echo" } else { "-echo" };
    let ok = std::process::Command::new("stty")
        .arg(arg)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = std::io::stderr().flush();
    ok && !on
}
