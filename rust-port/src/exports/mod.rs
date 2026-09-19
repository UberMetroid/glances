//! Exporter registry — one file per protocol.
//!
//! Each exporter exposes:
//!   - `NAME`: protocol identifier (used by CLI `--export`)
//!   - `Config`: per-exporter connection/path options
//!   - `write(snap, cfg) -> Result<()>`: send one snapshot
//!
//! The dispatch from `Args::export_targets` to a specific exporter lands in
//! M12-followup. For now each module ships a `register()` stub so the
//! registry is discoverable.

pub mod csv;
pub mod json;
pub mod influxdb;
pub mod statsd;
pub mod prometheus;
pub mod restful;

/// Registration stub — invoked by `main.rs` once at startup. Real Args
/// → exporter dispatch lands in M12-followup.
pub fn register() {
    let _ = csv::NAME;
    let _ = json::NAME;
    let _ = influxdb::NAME;
    let _ = statsd::NAME;
    let _ = prometheus::NAME;
    let _ = restful::NAME;
}