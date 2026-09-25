//! Connections plugin — TCP socket counts per state.
//!
//! Tallies the state column of /proc/net/tcp{,6} into stable buckets,
//! plus best-effort netfilter conntrack gauges (unreadable without
//! privileges — Null until a read succeeds).

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::plugins::ports;

pub const NAME: &str = "connections";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(ConnectionsPlugin::new()));
}

/// Exposed states, always present (zeroed) for a stable shape.
const KNOWN_STATES: &[&str] = &[
    "ESTABLISHED", "SYN_SENT", "SYN_RECV", "FIN_WAIT1", "FIN_WAIT2",
    "TIME_WAIT", "CLOSE", "CLOSE_WAIT", "LAST_ACK", "LISTEN",
    "CLOSING", "NEW_SYN_RECV",
];

/// Hex `st` column → state word (kernel tcp_states.h order).
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

/// The canonical zeroed shape.
pub fn empty_stats() -> Value {
    let mut m = BTreeMap::new();
    for s in KNOWN_STATES {
        m.insert((*s).into(), Value::Uint(0));
    }
    m.insert("UNKNOWN".into(), Value::Uint(0));
    m.insert("nf_conntrack_count".into(), Value::Null);
    m.insert("nf_conntrack_max".into(), Value::Null);
    Value::Object(m)
}

/// Tally rows into the buckets. UDP has no state — its rows never
/// count.
fn tally(rows: &[ports::NetRow], counts: &mut BTreeMap<String, u64>) {
    for r in rows {
        if r.family == "udp" {
            continue;
        }
        *counts.entry(st_name(&r.st).to_string()).or_insert(0) += 1;
    }
}

fn read_count(path: &str) -> Option<u64> {
    fs::read_to_string(path).ok().and_then(|s| s.trim().parse::<u64>().ok())
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
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }

    fn update(&mut self) -> Result<()> {
        // Fresh zeroed buckets every tick — never accumulate.
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for s in KNOWN_STATES {
            counts.insert((*s).into(), 0);
        }
        counts.insert("UNKNOWN".into(), 0);
        // Both TCP tables share state codes; missing files (older
        // kernels lack tcp6) degrade silently inside collect().
        let rows = ports::collect(&[("/proc/net/tcp", "tcp"), ("/proc/net/tcp6", "tcp6")]);
        tally(&rows, &mut counts);
        if let Some(obj) = self.base.stats.as_object_mut() {
            for (k, v) in &counts {
                obj.insert(k.clone(), Value::Uint(*v));
            }
            if let Some(c) = read_count("/proc/sys/net/netfilter/nf_conntrack_count") {
                obj.insert("nf_conntrack_count".into(), Value::Uint(c));
            }
            if let Some(mx) = read_count("/proc/sys/net/netfilter/nf_conntrack_max") {
                obj.insert("nf_conntrack_max".into(), Value::Uint(mx));
            }
        }
        Ok(())
    }
}

/// Tally rows into a fresh canonical map (the test entry point).
pub fn tally_rows(rows: &[ports::NetRow]) -> BTreeMap<String, u64> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for s in KNOWN_STATES {
        counts.insert((*s).into(), 0);
    }
    counts.insert("UNKNOWN".into(), 0);
    tally(rows, &mut counts);
    counts
}
