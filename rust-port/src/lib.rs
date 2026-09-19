//! glances-rs library entry point.
//!
//! Re-exports the public surface so integration tests in `qa/integration/`
//! can call into the same APIs the binary uses. See `src/main.rs` for the
//! binary entry point and `docs/ARCHITECTURE.md` for the overall design.

pub mod cli;
pub mod core;
pub mod exec;
pub mod exports;
pub mod net;
pub mod outputs;
pub mod platform;
pub mod plugins;
pub mod text;

#[cfg(test)]
pub mod qa;
