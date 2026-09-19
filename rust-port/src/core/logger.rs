//! Minimal stderr logger.
//!
//! M0 stub: writes to stderr with level prefix. M1+ will replace this with
//! a RotatingFileHandler-style logger at `$XDG_CACHE_HOME/glances/glances.log`
//! plus a console handler at CRITICAL by default (per the plan §2.4
//! `glances/logger.py` mapping).
//!
//! Std-only: no `log` crate. Just `eprintln!` + atomic log level.

use std::sync::atomic::{AtomicU8, Ordering};

/// Log levels, matching Python's `logging` module names (rough correspondence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
    Critical = 4,
}

static MIN_LEVEL: AtomicU8 = AtomicU8::new(0);

/// Initialize the logger. Idempotent.
pub fn init(debug: bool) {
    let lvl = if debug { Level::Debug as u8 } else { Level::Info as u8 };
    MIN_LEVEL.store(lvl, Ordering::Relaxed);
}

fn enabled(level: Level) -> bool {
    (level as u8) >= MIN_LEVEL.load(Ordering::Relaxed)
}

pub fn debug(msg: &str) {
    if enabled(Level::Debug) {
        eprintln!("[DEBUG] {}", msg);
    }
}

pub fn info(msg: &str) {
    if enabled(Level::Info) {
        eprintln!("[INFO]  {}", msg);
    }
}

pub fn warning(msg: &str) {
    if enabled(Level::Warning) {
        eprintln!("[WARN]  {}", msg);
    }
}

pub fn error(msg: &str) {
    if enabled(Level::Error) {
        eprintln!("[ERROR] {}", msg);
    }
}

pub fn critical(msg: &str) {
    if enabled(Level::Critical) {
        eprintln!("[CRIT]  {}", msg);
    }
}
