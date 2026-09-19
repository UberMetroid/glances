//! Help-text rendering.
//!
//! M0 stub: prints a minimal one-liner so `--help` doesn't crash.
//! Full help (matching Python Glances flag list per AC-14) lands in M2.

pub fn print_help() {
    println!("glances-rs — pure-stdlib Rust port of Glances");
    println!();
    println!("Usage: glances-rs [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -d, --debug              Enable debug logging");
    println!("  -t, --time SECONDS       Refresh interval (default 2)");
    println!("  -s, --server             Run as XML-RPC server");
    println!("  -c, --client HOST[:PORT] Run as XML-RPC client");
    println!("  -w, --webserver          Run REST API + Vue UI");
    println!("  -h, --help               Show this help message");
    println!("  -V, --version            Show version");
    println!();
    println!("Full flag list is implemented in milestone M2.");
}
