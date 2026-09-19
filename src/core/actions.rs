//! Action triggers — process kill / nice adjust.
//!
//! Mirrors the **safe** parts of `glances/actions.py`. Full mustache-templated
//! shell command actions (`GlancesActions.run`) land in M1-followup; the
//! M1 stub provides process control primitives that are obviously safe
//! (kill + renice) because they go through `std::process::Command` argv-only
//! with no shell.

use std::process::Command;

use super::error::{GlancesError, Result};

/// Resolve a system binary to an absolute path when it exists in the
/// conventional location — avoids PATH hijacking for privileged actions.
fn resolve_bin(cmd: &str) -> String {
    for dir in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        let full = format!("{}/{}", dir, cmd);
        if std::path::Path::new(&full).is_file() {
            return full;
        }
    }
    cmd.to_string()
}

/// Send a signal to a single PID. No shell, argv-only.
pub fn kill_pid(pid: u32, signal: i32) -> Result<()> {
    // SIGTERM = 15, SIGKILL = 9 (Linux). On Windows these are mapped.
    #[cfg(unix)]
    let status = {
        let mut c = Command::new(resolve_bin("kill"));
        c.arg(format!("-{}", signal)).arg(pid.to_string());
        c.status()
    };
    #[cfg(windows)]
    let status = {
        let mut c = Command::new("taskkill");
        if signal == 9 { c.arg("/F"); } else { c.arg("/T"); }
        c.arg("/PID").arg(pid.to_string());
        c.status()
    };
    status.map_err(GlancesError::Io).and_then(|s| {
        if s.success() { Ok(()) } else { Err(GlancesError::Other(format!("kill exit {:?}", s.code()))) }
    })
}

/// Adjust the nice value of a PID. +19 to -20 (Linux); Windows is best-effort.
#[cfg(unix)]
pub fn renice_pid(pid: u32, new_nice: i32) -> Result<()> {
    let status = Command::new(resolve_bin("renice"))
        .arg("-n").arg(new_nice.to_string())
        .arg("-p").arg(pid.to_string())
        .status()
        .map_err(GlancesError::Io)?;
    if status.success() { Ok(()) } else { Err(GlancesError::Other("renice failed".into())) }
}

#[cfg(not(unix))]
pub fn renice_pid(_pid: u32, _new_nice: i32) -> Result<()> {
    Err(GlancesError::Other("renice not supported on this platform".into()))
}
