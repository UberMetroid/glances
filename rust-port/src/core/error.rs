//! GlancesError — single error type for the whole crate.
//!
//! M0 stub: just `Io` + `Parse` + `Other(String)`. M1 will expand with
//! plugin-specific variants as needed. Uses `thiserror`-style manual impl
//! (we can't depend on `thiserror` per AC-1).

use std::fmt;
use std::io;

/// All errors in glances-rs go through this enum. Plugin authors add
/// their own variants if needed; core code uses the variants below.
#[derive(Debug)]
pub enum GlancesError {
    Io(io::Error),
    Parse(String),
    PluginDisabled(String),
    InvalidConfig(String),
    PermissionDenied(String),
    NotFound(String),
    Other(String),
}

impl fmt::Display for GlancesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GlancesError::Io(e) => write!(f, "io: {}", e),
            GlancesError::Parse(s) => write!(f, "parse: {}", s),
            GlancesError::PluginDisabled(s) => write!(f, "plugin disabled: {}", s),
            GlancesError::InvalidConfig(s) => write!(f, "invalid config: {}", s),
            GlancesError::PermissionDenied(s) => write!(f, "permission denied: {}", s),
            GlancesError::NotFound(s) => write!(f, "not found: {}", s),
            GlancesError::Other(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for GlancesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GlancesError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for GlancesError {
    fn from(e: io::Error) -> Self {
        GlancesError::Io(e)
    }
}

impl From<std::num::ParseIntError> for GlancesError {
    fn from(e: std::num::ParseIntError) -> Self {
        GlancesError::Parse(e.to_string())
    }
}

impl From<std::num::ParseFloatError> for GlancesError {
    fn from(e: std::num::ParseFloatError) -> Self {
        GlancesError::Parse(e.to_string())
    }
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, GlancesError>;
