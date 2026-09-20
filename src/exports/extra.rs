//! Overflow dispatch arms (keeps `dispatch.rs` under the line cap).
//!
//! New exporters land here; `dispatch()` tries this table first.

use crate::cli::args::Args;
use crate::core::error::Result;
use crate::exports::flatten::Field;

use super::dispatch::{hp_into, opt, set_opt, set_str};
use super::{duckdb, graph, graphite, influxdb, influxdb2, influxdb3, timescaledb, zeromq};
use crate::core::value::Value;

/// Dispatch one target. `None` = not owned by this table; the caller
/// falls through to the main match.
pub fn dispatch(name: &str, flat: &[Field<'_>], args: &Args) -> Option<Result<()>> {
    let r = match name {
        n if n == graphite::NAME => {
            let mut c = graphite::Config::default();
            hp_into(args, "graphite-host", &mut c.host, &mut c.port);
            set_str(args, "graphite-prefix", &mut c.prefix);
            graphite::write(flat, &c)
        }
        n if n == graph::NAME => {
            let mut c = graph::Config::default();
            set_str(args, "graph-path", &mut c.path);
            if let Some(Ok(w)) = opt(args, "graph-width").map(|v| v.parse()) { c.width = w; }
            if let Some(Ok(h)) = opt(args, "graph-height").map(|v| v.parse()) { c.height = h; }
            graph::write(flat, &c)
        }
        n if n == timescaledb::NAME => {
            let mut c = timescaledb::Config::default();
            hp_into(args, "timescaledb-host", &mut c.host, &mut c.port);
            set_str(args, "timescaledb-db", &mut c.db);
            set_str(args, "timescaledb-user", &mut c.user);
            set_str(args, "timescaledb-password", &mut c.password);
            set_str(args, "timescaledb-hostname", &mut c.hostname);
            timescaledb::write(flat, &c)
        }
        n if n == zeromq::NAME => {
            let mut c = zeromq::Config::default();
            hp_into(args, "zeromq-host", &mut c.host, &mut c.port);
            set_str(args, "zeromq-prefix", &mut c.prefix);
            zeromq::write(flat, &c)
        }
        n if n == duckdb::NAME => {
            let mut c = duckdb::Config::default();
            set_str(args, "duckdb-database", &mut c.database);
            duckdb::write(flat, &c)
        }
        _ => return None,
    };
    Some(r)
}

/// Influx-family arms (moved from `dispatch.rs` for the line cap).
/// Returns `None` when `name` is not an influx target.
pub fn dispatch_influx(
    name: &str,
    snap: &Value,
    flat: &[Field<'_>],
    args: &Args,
) -> Option<Result<()>> {
    let r = match name {
        n if n == influxdb::NAME => {
            let mut c = influxdb::Config::default();
            hp_into(args, "influxdb-host", &mut c.host, &mut c.port);
            set_str(args, "influxdb-db", &mut c.database);
            set_str(args, "influxdb-prefix", &mut c.prefix);
            set_opt(args, "influxdb-file", &mut c.file);
            set_opt(args, "influxdb-user", &mut c.user);
            set_opt(args, "influxdb-password", &mut c.password);
            if let Some(t) = opt(args, "influxdb-tags") {
                c.tags = influxdb::parse_tags(&t);
            }
            if let Some(h) = snap
                .as_object()
                .and_then(|o| o.get("system"))
                .and_then(|v| v.as_object())
                .and_then(|o| o.get("hostname"))
                .and_then(|v| v.as_str())
            {
                if !h.is_empty() { c.hostname = h.to_string(); }
            }
            influxdb::write(flat, &c)
        }
        n if n == influxdb2::NAME => {
            let mut c = influxdb2::Config::default();
            hp_into(args, "influxdb2-host", &mut c.host, &mut c.port);
            set_str(args, "influxdb2-org", &mut c.org);
            set_str(args, "influxdb2-bucket", &mut c.bucket);
            set_str(args, "influxdb2-token", &mut c.token);
            set_opt(args, "influxdb2-file", &mut c.file);
            influxdb2::write(flat, &c)
        }
        n if n == influxdb3::NAME => {
            let mut c = influxdb3::Config::default();
            hp_into(args, "influxdb3-host", &mut c.host, &mut c.port);
            set_str(args, "influxdb3-bucket", &mut c.bucket);
            set_str(args, "influxdb3-token", &mut c.token);
            set_opt(args, "influxdb3-file", &mut c.file);
            influxdb3::write(flat, &c)
        }
        _ => return None,
    };
    Some(r)
}
