//! Output sinks — the surfaces that present Glances data to the world.
//!
//! Each subdirectory owns one delivery channel: HTTP REST + SPA (web),
//! MCP (mcp), XML-RPC (xmlrpc), terminal UI (tui), and per-plugin API
//! handlers (api_handlers).

pub mod api_doc;
pub mod csv_stdout;
pub mod json_stdout;
pub mod mcp;
pub mod stdout_path;
pub mod tui;
pub mod web;
pub mod xmlrpc;
pub mod xmlrpc_transport;
