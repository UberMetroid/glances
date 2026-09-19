//! Connections plugin — counts of TCP sockets per connection state.
//!
//! Mirrors `glances/plugins/connections/__init__.py`. Reads /proc/net/tcp
//! and /proc/net/tcp6, tallies each row's state field (`st`), and adds
//! netfilter nf_conntrack counts when `/proc/sys/net/netfilter/` is
//! available (privileged).

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::plugins::ports;

pub const NAME: &str = "connections";

/// Wire-up entry point called by `plugins::register_all`. Mirrors the
/// pattern used by the other plugin modules (see e.g. `cpu::register`).
pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(ConnectionsPlugin::new()));
}

/// Canonical set of states we expose. We initialize every known state to
/// zero so the JSON shape is stable across hosts (some kernels rarely
/// emit e.g. CLOSING).
const KNOWN_STATES: &[&str] = &[
    "ESTABLISHED", "SYN_SENT", "SYN_RECV", "FIN_WAIT1", "FIN_WAIT2",
    "TIME_WAIT", "CLOSE", "CLOSE_WAIT", "LAST_ACK", "LISTEN",
    "CLOSING", "NEW_SYN_RECV",
];

/// Map hex `st` column → human state name.
fn st_name(st: &str) -> &'static str {
    match st {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        "0C" => "NEW_SYN_RECV",
        _ => "UNKNOWN",
    }
}

/// Build the empty stats object so the JSON shape is always identical.
pub fn empty_stats() -> Value {
    let mut m = BTreeMap::new();
    for s in KNOWN_STATES { m.insert((*s).into(), Value::Uint(0)); }
    m.insert("UNKNOWN".into(), Value::Uint(0));
    m.insert("nf_conntrack_count".into(), Value::Null);
    m.insert("nf_conntrack_max".into(), Value::Null);
    Value::Object(m)
}

/// Tally one parsed set of rows into the running counts map.
fn tally(rows: &[ports::NetRow], counts: &mut BTreeMap<String, u64>) {
    for r in rows {
        // Skip UDP — it has no real state and shouldn't inflate the totals.
        if r.family == "udp" { continue; }
        let key = st_name(&r.st).to_string();
        *counts.entry(key).or_insert(0) += 1;
    }
}

fn read_count(path: &str) -> Option<u64> {
    fs::read_to_string(path).ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
}

pub struct ConnectionsPlugin { base: GlancesPluginModel }

impl ConnectionsPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, empty_stats()) }
    }
}

impl Default for ConnectionsPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for ConnectionsPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }

    fn update(&mut self) -> Result<()> {
        // Start from the zeroed canonical shape — never accumulate across
        // ticks. Use BTreeMap for deterministic key order.
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for s in KNOWN_STATES { counts.insert((*s).into(), 0); }
        counts.insert("UNKNOWN".into(), 0);

        // /proc/net/tcp + /proc/net/tcp6 share the same state codes; tally
        // both into the same buckets. read_all_best_effort swallows IO
        // errors so missing /proc/net/tcp6 on older kernels is non-fatal.
        let paths = [
            ("/proc/net/tcp", "tcp"),
            ("/proc/net/tcp6", "tcp6"),
        ];
        let rows = ports::collect(&paths);
        tally(&rows, &mut counts);

        if let Some(obj) = self.base.stats.as_object_mut() {
            for (k, v) in &counts {
                obj.insert(k.clone(), Value::Uint(*v));
            }
            // nf_conntrack: best-effort, may be unreadable without CAP_NET_ADMIN.
            if let Some(c) = read_count("/proc/sys/net/netfilter/nf_conntrack_count") {
                obj.insert("nf_conntrack_count".into(), Value::Uint(c));
            }
            if let Some(m) = read_count("/proc/sys/net/netfilter/nf_conntrack_max") {
                obj.insert("nf_conntrack_max".into(), Value::Uint(m));
            }
        }
        Ok(())
    }
}

/// Exposed for tests: tally rows into a fresh canonical map and return it.
pub fn tally_rows(rows: &[ports::NetRow]) -> BTreeMap<String, u64> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for s in KNOWN_STATES { counts.insert((*s).into(), 0); }
    counts.insert("UNKNOWN".into(), 0);
    tally(rows, &mut counts);
    counts
}