//! Overflow dispatch arms (keeps `dispatch.rs` under the line cap).
//!
//! New exporters land here; `dispatch()` tries this table first.

use crate::cli::args::Args;
use crate::core::error::Result;
use crate::exports::flatten::Field;

use super::dispatch::{hp_into, opt, set_str};
use super::{duckdb, graph, graphite, timescaledb, zeromq};

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
