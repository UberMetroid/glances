//! Lint suite — enforces the hard constraints from plan §3.5.
//!
//! These tests are intentionally `#[ignore]`-friendly: they read the
//! source tree at test time and fail loudly if any rule is broken. They
//! are NOT run by default in `cargo test`; they run in CI and in
//! `cargo test -- --include-ignored`.

pub mod no_crates;
pub mod line_cap;
pub mod ownership;
pub mod unsafe_allowlist;
pub mod no_shell;
