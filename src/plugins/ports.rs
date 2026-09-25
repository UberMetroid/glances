//! Ports plugin — list of LISTEN / ESTABLISHED TCP sockets and all UDP
//! sockets. Mirrors `glances/plugins/ports/__init__.py`.
//!
//! Reads `/proc/net/tcp`, `/proc/net/tcp6`, and `/proc/net/udp`. Keeps
//! only TCP state `0A` (LISTEN) and `01` (ESTABLISHED). UDP has no
//! connection state, so all UDP entries are kept. Output is capped at
//! `MAX_ENTRIES` rows.

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "ports";

/// Wire-up entry point called by `plugins::register_all`.
pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(PortsPlugin::new()));
}

/// Hard cap on output rows. Glances uses a similar threshold.
const MAX_ENTRIES: usize = 1000;

/// Hex state → state name (TCP only). UDP has no state column.
pub fn tcp_state_name(st: &str) -> &'static str {
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

/// Decode a hex IPv4 address (8 hex chars, little-endian byte order).
/// `0100007F` → `127.0.0.1` because the kernel emits each IP byte in
/// reverse (network) order.
pub fn decode_ipv4(hex: &str) -> Option<String> {
    if hex.len() != 8 { return None; }
    let bytes = crate::core::hex::decode(hex)?;
    Some(format!("{}.{}.{}.{}", bytes[3], bytes[2], bytes[1], bytes[0]))
}

/// Decode a hex IPv6 address (32 hex chars). The kernel stores each
/// 32-bit word in host (little-endian) order, so every 4-byte group
/// must be byte-reversed — `::1` arrives as `...0000000001000000`.
pub fn decode_ipv6(hex: &str) -> Option<String> {
    if hex.len() != 32 { return None; }
    let bytes = crate::core::hex::decode(hex)?;
    let mut w = [0u8; 16];
    for g in 0..4 {
        for b in 0..4 {
            w[g * 4 + b] = bytes[g * 4 + (3 - b)];
        }
    }
    let mut out = String::with_capacity(39);
    for i in 0..8 {
        if i > 0 { out.push(':'); }
        out.push_str(&format!("{:x}", ((w[i * 2] as u16) << 8) | (w[i * 2 + 1] as u16)));
    }
    Some(out)
}

/// Split "IP_HEX:PORT_HEX" into (decoded_ip, decoded_port).
pub fn split_addr(addr: &str) -> (String, u64) {
    let Some(colon) = addr.find(':') else { return (addr.to_string(), 0); };
    let ip_hex = &addr[..colon];
    let port_hex = &addr[colon + 1..];
    let ip = match ip_hex.len() {
        8 => decode_ipv4(ip_hex).unwrap_or_else(|| ip_hex.to_string()),
        32 => decode_ipv6(ip_hex).unwrap_or_else(|| ip_hex.to_string()),
        _ => ip_hex.to_string(),
    };
    let port = crate::core::hex::decode(port_hex)
        .and_then(|b| if b.len() == 2 { Some(u16::from_be_bytes([b[0], b[1]]) as u64) } else { None })
        .unwrap_or(0);
    (ip, port)
}

/// One row in /proc/net/{tcp,tcp6,udp}.
#[derive(Debug, Default, Clone)]
pub struct NetRow {
    pub sl: String,
    pub local_address: String,
    pub rem_address: String,
    pub st: String,
    pub tx_queue: String,
    pub rx_queue: String,
    pub tr: String,
    pub tm_when: String,
    pub retrnsmt: String,
    pub uid: String,
    pub timeout: String,
    pub inode: String,
    pub family: &'static str,
}

/// Parse `/proc/net/{tcp,tcp6,udp}` text. `family` is stored in each
/// row so downstream code can branch without re-deriving it.
///
/// Modern Linux kernels (≥ ~2.6.32) emit 11 essential whitespace-
/// separated tokens for `tcp`/`tcp6` (tx_queue and rx_queue are combined
/// into one token as `tx_queue:rx_queue`, same for `tr:tm->when`).
/// Very old kernels emitted 12 separate tokens (one per historical
/// column). We accept either format and adjust the mapping.
pub fn parse(text: &str, family: &'static str) -> Result<Vec<NetRow>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 { continue; } // skip "  sl local_address ..." header
        let line = line.trim();
        if line.is_empty() { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Modern format: 11 tokens where tx_queue and rx_queue share one
        // token as "tx_queue:rx_queue", same for tr:tm->when.
        // Old format: 12 tokens (split).
        // We detect by checking if parts[4] looks like "X:Y".
        if parts.len() < 11 { continue; }
        let combined = parts[4].contains(':');
        // Old (split-column) format needs 12 tokens; an 11-token line
        // without combined queues would panic on parts[11] below.
        if !combined && parts.len() < 12 { continue; }
        let (txq, rxq, tr, tm) = if combined {
            // Modern: parts[4]="tx:rx", parts[5]="tr:tm", parts[6]=retrnsmt
            //        parts[7]=uid, parts[8]=timeout, parts[9]=inode
            let (txq, rxq) = parts[4].split_once(':').unwrap_or((parts[4], "0"));
            let (tr, tm) = parts[5].split_once(':').unwrap_or((parts[5], "0"));
            (txq.to_string(), rxq.to_string(), tr.to_string(), tm.to_string())
        } else {
            // Old: parts[4]=tx, parts[5]=rx, parts[6]=tr, parts[7]=tm
            //      parts[8]=retrnsmt, parts[9]=uid, parts[10]=timeout, parts[11]=inode
            (parts[4].to_string(), parts[5].to_string(), parts[6].to_string(), parts[7].to_string())
        };
        let (retrnsmt, uid, timeout, inode) = if combined {
            (parts[6].to_string(), parts[7].to_string(), parts[8].to_string(), parts[9].to_string())
        } else {
            (parts[8].to_string(), parts[9].to_string(), parts[10].to_string(), parts[11].to_string())
        };
        out.push(NetRow {
            sl: parts[0].trim_end_matches(':').to_string(),
            local_address: parts[1].to_string(),
            rem_address: parts[2].to_string(),
            st: parts[3].to_string(),
            tx_queue: txq,
            rx_queue: rxq,
            tr,
            tm_when: tm,
            retrnsmt,
            uid,
            timeout,
            inode,
            family,
        });
    }
    Ok(out)
}

fn row_to_value(r: &NetRow) -> Value {
    let (lip, lp) = split_addr(&r.local_address);
    let (rip, rp) = split_addr(&r.rem_address);
    let mut obj = BTreeMap::new();
    obj.insert("sl".into(), Value::String(r.sl.clone()));
    obj.insert("family".into(), Value::String(r.family.to_string()));
    obj.insert("local_address".into(), Value::String(r.local_address.clone()));
    obj.insert("local_ip".into(), Value::String(lip));
    obj.insert("local_port".into(), Value::Uint(lp));
    obj.insert("rem_address".into(), Value::String(r.rem_address.clone()));
    obj.insert("remote_ip".into(), Value::String(rip));
    obj.insert("remote_port".into(), Value::Uint(rp));
    obj.insert("st".into(), Value::String(r.st.clone()));
    obj.insert("state".into(), Value::String(tcp_state_name(&r.st).to_string()));
    obj.insert("tx_queue".into(), Value::String(r.tx_queue.clone()));
    obj.insert("rx_queue".into(), Value::String(r.rx_queue.clone()));
    obj.insert("tr".into(), Value::String(r.tr.clone()));
    obj.insert("tm_when".into(), Value::String(r.tm_when.clone()));
    obj.insert("retrnsmt".into(), Value::String(r.retrnsmt.clone()));
    obj.insert("uid".into(), Value::String(r.uid.clone()));
    obj.insert("timeout".into(), Value::String(r.timeout.clone()));
    obj.insert("inode".into(), Value::String(r.inode.clone()));
    Value::Object(obj)
}

/// Collect rows from a list of `(path, family)` tuples. Best-effort:
/// missing/unreadable paths are skipped silently.
pub fn collect(paths: &[(&'static str, &'static str)]) -> Vec<NetRow> {
    let mut out = Vec::new();
    for (path, family) in paths {
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(rows) = parse(&text, family) {
                out.extend(rows);
            }
    }
    out
}

pub struct PortsPlugin { base: GlancesPluginModel }

impl PortsPlugin {
    pub fn register(stats: &crate::core::stats::GlancesStats) {
        stats.register(Box::new(PortsPlugin::new()));
    }
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

impl Default for PortsPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for PortsPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn get_key(&self) -> Option<&'static str> { Some("inode") }

    fn update(&mut self) -> Result<()> {
        // tcp first, tcp6 second, udp last — preserves ordering.
        let paths: [(&'static str, &'static str); 3] = [
            ("/proc/net/tcp", "tcp"),
            ("/proc/net/tcp6", "tcp6"),
            ("/proc/net/udp", "udp"),
        ];
        let mut rows = collect(&paths);
        rows.retain(|r| {
            // UDP has no real state column; keep all UDP entries.
            if r.family == "udp" { return true; }
            // TCP: keep LISTEN (0A) and ESTABLISHED (01) only.
            r.st == "0A" || r.st == "01"
        });

        let cap = rows.len().min(MAX_ENTRIES);
        let mut arr: Vec<Value> = Vec::with_capacity(cap);
        for r in rows.iter().take(cap) {
            arr.push(row_to_value(r));
        }
        self.base.stats = Value::Array(arr);
        // We swallow individual read failures inside `collect()` because
        // /proc/net/{tcp6,udp} may be missing on some kernels.
        Ok(())
    }
}