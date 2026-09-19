//! Output sinks — the surfaces that present Glances data to the world.
//!
//! Each subdirectory owns one delivery channel: HTTP REST + SPA (web),
//! MCP (mcp), terminal UI (tui), and per-plugin API handlers (api_handlers).
//! M14 lands `web/`. M15a lands the `tui/` scaffolding (stdout renderer).

pub mod api_doc;
pub mod csv_stdout;
pub mod json_stdout;
// pub mod tui;  // TUI scaffolding references files that don't exist on disk.
pub mod web;
