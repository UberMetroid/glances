//! glances-rs binary entry point.
//!
//! M0 skeleton: parses CLI args, dispatches to a mode handler.
//! Subsequent milestones replace the body with real mode dispatch
//! (standalone / web / XML-RPC / MCP / stdout-csv / stdout-json).
//! See `docs/ARCHITECTURE.md` §5.

use glances_rs::cli::args::{parse_args, Mode};
use glances_rs::cli::help;
use glances_rs::core::logger;

fn main() {
    let args = parse_args();
    logger::init(args.debug);
    logger::info(&format!(
        "glances-rs starting (mode={:?}, refresh={}s)",
        args.mode, args.refresh_time
    ));
    match args.mode {
        Mode::Help => {
            help::print_help();
        }
        Mode::Version => {
            println!("glances-rs 0.1.0 (M0 skeleton)");
        }
        _ => {
            println!(
                "glances-rs: mode {:?} not yet implemented (later milestone; see plan)",
                args.mode
            );
        }
    }
}
