//! Hardened subprocess wrapper — argv-only, no shell.
//!
//! M0 placeholder. M1-followup adds `safe_run.rs` with command-binary
//! allowlists and env scrubbing (per plan §3.5 hard constraint #4).

pub use std::process::Command as SafeCommand;
