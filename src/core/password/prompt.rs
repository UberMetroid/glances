//! Startup credential prompts: servers define logins, clients enter them.
//!
//! Without prompt flags the defaults apply (`glances`/empty) and stdin
//! is never touched, so pipes and timed runs never block.

use super::PasswordFile;
use crate::cli::args::{Args, Mode};

/// Mode dispatch: the web server defines credentials, the client only
/// enters them for the session; every other mode skips auth entirely.
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
    // A prompted username always needs a password alongside it.
    if args.username_prompt {
        args.password_prompt = true;
    }
    let username = if args.username_prompt {
        let ask = if server_side { "Enter new username: " } else { "Enter username: " };
        read_line(ask)
    } else if let Some(u) = args.username_used.clone() {
        u
    } else {
        "glances".to_string()
    };
    args.username = Some(username.clone());
    if !args.password_prompt && args.username_used.is_none() {
        args.password = Some(String::new());
        return;
    }
    if !server_side {
        args.password = Some(read_password("Enter password: "));
        return;
    }
    loop {
        let first = read_password("Enter new password: ");
        let second = read_password("Confirm password: ");
        if first != second {
            crate::core::logger::warning("passwords do not match, try again");
            continue;
        }
        pw.set(&username, &first);
        if let Err(e) = pw.save() {
            crate::core::logger::warning(&format!("could not save password file: {e}"));
        }
        break;
    }
    args.password = None;
}

/// Prompt on stderr, read one stdin line (EOF or error reads empty).
fn read_line(desc: &str) -> String {
    use std::io::Write as _;
    let _ = write!(std::io::stderr(), "{desc}");
    let _ = std::io::stderr().flush();
    let mut buf = String::new();
    match std::io::stdin().read_line(&mut buf) {
        Ok(_) => buf.trim_end_matches(['\r', '\n']).to_string(),
        Err(_) => String::new(),
    }
}

/// Password prompt with echo off, getpass-style. Echo toggles through
/// `stty` (argv-only, restored right after the read); when that fails
/// the read stays visible rather than failing startup.
fn read_password(desc: &str) -> String {
    use std::io::Write as _;
    let hidden = set_echo(false);
    let pw = read_line(desc);
    if hidden {
        set_echo(true);
    }
    let _ = writeln!(std::io::stderr());
    pw.trim_end_matches(['\r', '\n']).to_string()
}

/// Toggle TTY echo via `stty`. Reports whether echo ended up disabled,
/// so the caller restores only what it changed.
fn set_echo(on: bool) -> bool {
    use std::io::Write as _;
    let ok = std::process::Command::new("stty")
        .arg(if on { "echo" } else { "-echo" })
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = std::io::stderr().flush();
    ok && !on
}
