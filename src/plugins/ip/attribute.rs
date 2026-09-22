//! Local-IP → interface attribution for the `network` plugin.
//!
//! Route-table subnets own most addresses, but policy-routed
//! tunnels (tailscale) and loopback leave no main-table route, so:
//! longest-prefix route match first, 127/8 to `lo`, then exact 1:1
//! elimination (one homeless IP and one address-less connected
//! interface must belong together). Anything ambiguous stays
//! unattributed — never guessed.

use std::collections::{BTreeMap, HashSet};

use super::route::{ipv4_to_u32, RouteRow};

/// Attribute every local IP to at most one interface. `up` holds
/// the connected interface names (elimination only considers those).
pub fn attribute_ips(
    up: &[String],
    routes: &[RouteRow],
    local_ips: &[String],
) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut rest: Vec<&String> = Vec::new();
    for ip in local_ips {
        if !seen.insert(ip) { continue; }
        // 127/8 is loopback by kernel reservation — never another
        // interface's, and never elimination fodder.
        if ip.starts_with("127.") {
            if up.iter().any(|n| n == "lo") {
                out.entry("lo".to_string()).or_default().push(ip.clone());
            }
            continue;
        }
        let v = match ipv4_to_u32(ip) {
            Some(v) => v,
            None => continue,
        };
        let mut best: Option<(&RouteRow, u32)> = None;
        for r in routes {
            if r.dest == 0 || v & r.mask != r.dest { continue; }
            let len = r.mask.count_ones();
            if best.map_or(true, |(_, b)| len > b) { best = Some((r, len)); }
        }
        match best {
            Some((r, _)) => out.entry(r.iface.clone()).or_default().push(ip.clone()),
            None => rest.push(ip),
        }
    }
    if rest.len() == 1 {
        let bare: Vec<&String> = up.iter().filter(|n| !out.contains_key(*n)).collect();
        if bare.len() == 1 {
            out.entry(bare[0].to_string()).or_default().push(rest[0].clone());
        }
    }
    out
}
