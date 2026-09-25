//! Process control: signal delivery and priority changes.
//!
//! Both run argv-only with resolved binary paths (no shell, no PATH
//! lookup at exec time). Linux-only.

mod run;

pub use run::{sanitize_value, GlancesActions};

use std::process::Command;

use super::error::{GlancesError, Result};

/// Resolve a system binary to its absolute path (first hit wins),
/// falling back to the bare name. Avoids PATH hijacking.
fn resolve_bin(cmd: &str) -> String {
    ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        .into_iter()
        .map(|d| format!("{d}/{cmd}"))
        .find(|full| std::path::Path::new(full).is_file())
        .unwrap_or_else(|| cmd.to_string())
}

/// Deliver `signal` to one PID (`kill -<signal> <pid>`).
pub fn kill_pid(pid: u32, signal: i32) -> Result<()> {
    let status = Command::new(resolve_bin("kill"))
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()
        .map_err(GlancesError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(GlancesError::Other(format!("kill exit {:?}", status.code())))
    }
}

/// Set a PID's nice value (`renice -n <v> -p <pid>`).
pub fn renice_pid(pid: u32, new_nice: i32) -> Result<()> {
    let status = Command::new(resolve_bin("renice"))
        .arg("-n")
        .arg(new_nice.to_string())
        .arg("-p")
        .arg(pid.to_string())
        .status()
        .map_err(GlancesError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(GlancesError::Other("renice failed".into()))
    }
}
