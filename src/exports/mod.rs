//! Exporter registry + `--export` dispatch.
//!
//! Each exporter exposes:
//!   - `NAME`: protocol identifier (used by CLI `--export`)
//!   - `Config`: per-exporter connection/path options
//!   - `write(fields, cfg) -> Result<()>`: send one flattened snapshot
//!
//! `write_targets` fans one snapshot out to every `--export <name>`
//! target. Per-exporter options come from `Args::export_opts` (the
//! values of `--export-<opt>` flags, stored without the prefix).
//! Snapshot-shaped exporters (`kafka`, `restful`, `json`) also receive
//! the raw snapshot.

pub mod cassandra;
pub mod clickhouse;
pub mod couchdb;
pub mod csv;
mod dispatch;
pub mod duckdb;
pub mod elasticsearch;
pub mod flatten;
pub mod graph;
pub mod graphite;
pub mod influxdb;
pub mod influxdb2;
pub mod influxdb3;
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
pub mod timescaledb;
pub mod zeromq;

use crate::cli::args::Args;
use crate::core::value::Value;

/// All `--export` target names in module order (`--modules-list`).
pub const EXPORTERS: &[&str] = &[
    cassandra::NAME,
    clickhouse::NAME,
    couchdb::NAME,
    csv::NAME,
    duckdb::NAME,
    elasticsearch::NAME,
    graph::NAME,
    graphite::NAME,
    influxdb::NAME,
    influxdb2::NAME,
    influxdb3::NAME,
    json::NAME,
    kafka::NAME,
    mongodb::NAME,
    mqtt::NAME,
    nats::NAME,
    opentsdb::NAME,
    prometheus::NAME,
    rabbitmq::NAME,
    restful::NAME,
    riemann::NAME,
    statsd::NAME,
    timescaledb::NAME,
    zeromq::NAME,
];

/// Names of all built-in exporters (`--modules-list`).
pub fn exporter_names() -> Vec<&'static str> {
    EXPORTERS.to_vec()
}

/// Registration hook — kept for parity with `plugins::register_all`.
/// Exporter dispatch happens per refresh tick via `write_targets`.
pub fn register() {
    let _ = cassandra::NAME;
    let _ = clickhouse::NAME;
    let _ = couchdb::NAME;
    let _ = csv::NAME;
    let _ = duckdb::NAME;
    let _ = elasticsearch::NAME;
    let _ = graph::NAME;
    let _ = graphite::NAME;
    let _ = influxdb::NAME;
    let _ = influxdb2::NAME;
    let _ = influxdb3::NAME;
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
    let _ = timescaledb::NAME;
    let _ = zeromq::NAME;
}

/// Send `snap` to every `--export` target. Per-target errors are logged
/// and skipped — a dead endpoint must not kill the refresh loop (same
/// policy as Python Glances' export loop).
///
/// `keys` maps plugin name → its `get_key` element field, used to name
/// per-element series for array plugins (`network.eth0`, `fs./`, …).
pub fn write_targets(
    snap: &Value,
    args: &Args,
    keys: &std::collections::HashMap<String, &'static str>,
) {
    let flat = flatten::collect(snap, keys);
    for target in &args.export_targets {
        if let Err(e) = dispatch::dispatch(target, snap, &flat, args) {
            crate::core::logger::warning(&format!("export {} failed: {}", target, e));
        }
    }
}
