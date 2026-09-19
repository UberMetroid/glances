//! Exporter registry + `--export` dispatch.
//!
//! Each exporter exposes:
//!   - `NAME`: protocol identifier (used by CLI `--export`)
//!   - `Config`: per-exporter connection/path options
//!   - `write(snap, cfg) -> Result<()>`: send one snapshot
//!
//! `write_targets` fans one snapshot out to every `--export <name>`
//! target. Per-exporter options come from `Args::export_opts` (the
//! values of `--export-<opt>` flags, stored without the prefix).

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

use crate::cli::args::Args;
use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;

/// Registration hook — kept for parity with `plugins::register_all`.
/// Exporter dispatch happens per refresh tick via `write_targets`.
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

/// Send `snap` to every `--export` target. Per-target errors are logged
/// and skipped — a dead endpoint must not kill the refresh loop (same
/// policy as Python Glances' export loop).
pub fn write_targets(snap: &Value, args: &Args) {
    for target in &args.export_targets {
        if let Err(e) = dispatch(target, snap, args) {
            crate::core::logger::warning(&format!("export {} failed: {}", target, e));
        }
    }
}

/// Look up the value of `--export-<key>` (e.g. `opt("mqtt-server")`).
fn opt<'a>(args: &'a Args, key: &str) -> Option<&'a str> {
    args.export_opts.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Parse `host[:port]` — bare host keeps `default_port`.
fn split_host_port(s: &str, default_port: u16) -> (String, u16) {
    match s.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() => (h.to_string(), p.parse().unwrap_or(default_port)),
        _ => (s.to_string(), default_port),
    }
}

/// Build the host/port pair for `key`, falling back to the Config default.
fn hp(args: &Args, key: &str, default_port: u16, default_host: &str) -> (String, u16) {
    opt(args, key).map(|s| split_host_port(s, default_port))
        .unwrap_or_else(|| (default_host.to_string(), default_port))
}

fn dispatch(name: &str, snap: &Value, args: &Args) -> Result<()> {
    match name {
        n if n == csv::NAME => {
            let mut c = csv::Config::default();
            if let Some(p) = opt(args, "csv-file") { c.path = p.to_string(); }
            csv::write(snap, &c)
        }
        n if n == json::NAME => {
            let mut c = json::Config::default();
            if let Some(p) = opt(args, "json-file") { c.path = p.to_string(); }
            json::write(snap, &c)
        }
        n if n == influxdb::NAME => {
            let d = influxdb::Config::default();
            let (host, port) = hp(args, "influxdb-host", d.port, &d.host);
            influxdb::write(snap, &influxdb::Config { host, port, ..d })
        }
        n if n == influxdb2::NAME => {
            let mut c = influxdb2::Config::default();
            let (h, p) = hp(args, "influxdb2-host", c.port, &c.host);
            c.host = h; c.port = p;
            if let Some(v) = opt(args, "influxdb2-org") { c.org = v.to_string(); }
            if let Some(v) = opt(args, "influxdb2-bucket") { c.bucket = v.to_string(); }
            if let Some(v) = opt(args, "influxdb2-token") { c.token = v.to_string(); }
            influxdb2::write(snap, &c)
        }
        n if n == statsd::NAME => {
            let d = statsd::Config::default();
            let (host, port) = hp(args, "statsd-host", d.port, &d.host);
            statsd::write(snap, &statsd::Config { host, port, ..d })
        }
        n if n == riemann::NAME => {
            let d = riemann::Config::default();
            let (host, port) = hp(args, "riemann-host", d.port, &d.host);
            riemann::write(snap, &riemann::Config { host, port, ..d })
        }
        n if n == kafka::NAME => {
            let d = kafka::Config::default();
            let (host, port) = hp(args, "kafka-bootstrap", d.port, &d.host);
            kafka::write(snap, &kafka::Config { host, port, ..d })
        }
        n if n == nats::NAME => {
            let d = nats::Config::default();
            let (host, port) = hp(args, "nats-server", d.port, &d.host);
            nats::write(snap, &nats::Config { host, port, ..d })
        }
        n if n == mqtt::NAME => {
            let mut c = mqtt::Config::default();
            let (h, p) = hp(args, "mqtt-server", c.port, &c.host);
            c.host = h; c.port = p;
            if let Some(v) = opt(args, "mqtt-user") { c.username = Some(v.to_string()); }
            if let Some(v) = opt(args, "mqtt-password") { c.password = Some(v.to_string()); }
            mqtt::write(snap, &c)
        }
        n if n == mongodb::NAME => {
            let d = mongodb::Config::default();
            let (host, port) = hp(args, "mongodb-uri", d.port, &d.host);
            mongodb::write(snap, &mongodb::Config { host, port, ..d })
        }
        n if n == cassandra::NAME => {
            let d = cassandra::Config::default();
            let (host, port) = hp(args, "cassandra-host", d.port, &d.host);
            cassandra::write(snap, &cassandra::Config { host, port, ..d })
        }
        n if n == clickhouse::NAME => {
            let d = clickhouse::Config::default();
            let (host, port) = hp(args, "clickhouse-host", d.port, &d.host);
            clickhouse::write(snap, &clickhouse::Config { host, port, ..d })
        }
        n if n == couchdb::NAME => {
            let d = couchdb::Config::default();
            let (host, port) = hp(args, "couchdb-host", d.port, &d.host);
            couchdb::write(snap, &couchdb::Config { host, port, ..d })
        }
        n if n == elasticsearch::NAME => {
            let d = elasticsearch::Config::default();
            let (host, port) = hp(args, "elasticsearch-host", d.port, &d.host);
            elasticsearch::write(snap, &elasticsearch::Config { host, port, ..d })
        }
        n if n == opentsdb::NAME => {
            let d = opentsdb::Config::default();
            let (host, port) = hp(args, "opentsdb-host", d.port, &d.host);
            opentsdb::write(snap, &opentsdb::Config { host, port, ..d })
        }
        n if n == rabbitmq::NAME => {
            let d = rabbitmq::Config::default();
            // amqp://host:port/vhost — take host[:port], ignore vhost part.
            let url = opt(args, "rabbitmq-url").unwrap_or_default();
            let hostpart = url.strip_prefix("amqp://").unwrap_or(url);
            let hostpart = hostpart.split('/').next().unwrap_or("");
            let (host, port) = if hostpart.is_empty() {
                (d.host.clone(), d.port)
            } else {
                split_host_port(hostpart, d.port)
            };
            rabbitmq::write(snap, &rabbitmq::Config { host, port, ..d })
        }
        n if n == restful::NAME => {
            let d = restful::Config::default();
            let url = opt(args, "restful-url").unwrap_or_default();
            let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")).unwrap_or(url);
            let mut it = rest.splitn(2, '/');
            let (host, port) = if rest.is_empty() {
                (d.host.clone(), d.port)
            } else {
                split_host_port(it.next().unwrap_or(""), d.port)
            };
            let path = it.next().map(|p| format!("/{}", p)).unwrap_or_else(|| d.path.clone());
            restful::write(snap, &restful::Config { host, port, path, ..d })
        }
        n if n == prometheus::NAME => prometheus::write(snap, &prometheus::Config::default()),
        other => Err(GlancesError::Parse(format!("unknown export target: {}", other))),
    }
}
