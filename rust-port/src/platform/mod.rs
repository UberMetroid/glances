//! OS-specific metric collection. Cross-platform plugins reach into here
//! using `#[cfg(target_os = "...")]` to pick the right implementation.
//!
//! On non-Linux hosts the linux module is still compiled (it's pure
//! /proc + /sys text parsers) so tests can run with the fixtures; the
//! `read()` functions simply fail at runtime when /proc/stat is missing.

pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;
