//! CLI module — handwritten argument parser.
//!
//! See `docs/ARCHITECTURE.md` §3.1 (crate layout) and §5 (startup flow).
//! The parser is split across `args.rs`, `parse.rs`, `flags.rs`, and
//! `help.rs` to stay under the 256-line-per-file cap.

pub mod args;
pub mod parse;
pub mod flags;
pub mod help;
pub mod modes;

pub use args::{parse_args, Args, Mode};
