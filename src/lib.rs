//! glances-rs library entry point.
//!
//! Re-exports the public surface so integration tests in `src/qa/integration/`
//! can call into the same APIs the binary uses. See `src/main.rs` for the
//! binary entry point, `Cargo.toml` for the package metadata, and `src/qa/`
//! for the test layout.

pub mod cli;
pub mod core;
pub mod outputs;
pub mod platform;
pub mod plugins;

#[cfg(test)]
pub mod qa;
