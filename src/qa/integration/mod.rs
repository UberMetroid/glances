//! End-to-end smoke tests — one file per scenario.

pub mod binary_runs;
pub mod cli_help;
pub mod cli_parse;
pub mod installer;
pub mod plugins_smoke;
// pub mod tui_standalone_smoke;  // references unbuilt tui/ scaffolding
pub mod web_api_smoke;
