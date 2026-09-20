//! Core types — the cross-cutting foundation for everything else.
//!
//! M1 implementation lives here. See `docs/ARCHITECTURE.md` §3.1.

pub mod error;
pub mod logger;
pub mod value;
pub mod plugin;
pub mod stats;
pub(crate) mod stats_actions;
pub mod history;
pub mod threshold;
pub mod timer;
pub mod config;
pub mod config_dir;
pub mod alerts;
pub mod alert_views;
pub mod filter;
pub mod actions;
pub mod events;
pub mod password;
pub mod pbkdf2;
pub mod hex;
pub mod sha256;
