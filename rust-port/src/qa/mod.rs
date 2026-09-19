//! QA harness root module. Lives inside `src/qa/` so it has access to the
//! crate's private items. Each subdirectory contains `#[cfg(test)] mod`
//! files that are included here.
//!
//! Folder layout (all `#[cfg(test)]` only):
//! - `harness/` — test utilities (StdoutCapture, fake_proc, etc.)
//! - `unit/` — per-module unit tests
//! - `integration/` — end-to-end smoke tests
//! - `edge/` — edge-case suites
//! - `lint/` — code-shape invariants (no_crates, line_cap, unsafe_allowlist)
//! - `fixtures/` — sample inputs (text files)
//! - `snapshot/` — golden outputs
//! - `fuzz/` — cargo-fuzz harness sources

#[cfg(test)]
pub mod harness;
#[cfg(test)]
pub mod unit;
#[cfg(test)]
pub mod integration;
#[cfg(test)]
pub mod edge;
#[cfg(test)]
pub mod lint;
