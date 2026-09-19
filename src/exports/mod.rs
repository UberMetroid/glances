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

pub mod cassandra;
pub mod clickhouse;
pub mod couchdb;
pub mod csv;
pub mod elasticsearch;
pub mod influxdb;
pub mod influxdb2;
pub mod json;
pub mod kafka;
pub mod mongodb;
pub mod mqtt;
pub mod nats;
pub mod opentsdb;
pub mod prometheus;
pub mod rabbitmq;
pub mod restful;
pub mod riemann;
pub mod statsd;

/// Registration stub — invoked by `main.rs` once at startup. Real Args
/// → exporter dispatch lands in M12-followup.
pub fn register() {
    let _ = cassandra::NAME;
    let _ = clickhouse::NAME;
    let _ = couchdb::NAME;
    let _ = csv::NAME;
    let _ = elasticsearch::NAME;
    let _ = influxdb::NAME;
    let _ = influxdb2::NAME;
    let _ = json::NAME;
    let _ = kafka::NAME;
    let _ = mongodb::NAME;
    let _ = mqtt::NAME;
    let _ = nats::NAME;
    let _ = opentsdb::NAME;
    let _ = prometheus::NAME;
    let _ = rabbitmq::NAME;
    let _ = restful::NAME;
    let _ = riemann::NAME;
    let _ = statsd::NAME;
}