//! Ports plugin — socket table: LISTEN/ESTABLISHED TCP plus all UDP.
//!
//! Parses the three /proc/net tables into rows carrying raw and
//! decoded addresses, capped at 1000 rows (tcp, tcp6, udp order).

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "ports";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(PortsPlugin::new()));
}

/// Output row cap.
const MAX_ENTRIES: usize = 1000;

/// Hex state → word (kernel tcp_states.h order; UDP never consults
/// this — it has no state column).
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

/// 8-hex-digit IPv4, byte-reversed by the kernel (`0100007F` is
/// 127.0.0.1).
pub fn decode_ipv4(hex: &str) -> Option<String> {
    if hex.len() != 8 {
        return None;
    }
    let bytes = crate::core::hex::decode(hex)?;
    Some(format!("{}.{}.{}.{}", bytes[3], bytes[2], bytes[1], bytes[0]))
}

/// 32-hex-digit IPv6, each 32-bit word byte-reversed (`::1` arrives
/// as `...0000000001000000`). Groups print uncompressed, lowercase.
pub fn decode_ipv6(hex: &str) -> Option<String> {
    if hex.len() != 32 {
        return None;
    }
    let bytes = crate::core::hex::decode(hex)?;
    let mut w = [0u8; 16];
    for g in 0..4 {
        for b in 0..4 {
            w[g * 4 + b] = bytes[g * 4 + (3 - b)];
        }
    }
    let groups: Vec<String> =
        (0..8).map(|i| format!("{:x}", ((w[i * 2] as u16) << 8) | w[i * 2 + 1] as u16)).collect();
    Some(groups.join(":"))
}

/// Split `IP_HEX:PORT_HEX` into decoded (ip, port). Malformed halves
/// fall back to raw text / port 0 rather than dropping the row.
pub fn split_addr(addr: &str) -> (String, u64) {
    let Some(colon) = addr.find(':') else {
        return (addr.to_string(), 0);
    };
    let (ip_hex, port_hex) = (&addr[..colon], &addr[colon + 1..]);
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

/// One parsed table row.
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

/// Parse one table's text. Accepts the modern 11-token layout (queue
/// pairs combined as `tx:rx` / `tr:tm`) and the legacy 12-token
/// layout, told apart by the colon in token 4; short lines skip.
pub fn parse(text: &str, family: &'static str) -> Result<Vec<NetRow>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 11 {
            continue;
        }
        let combined = parts[4].contains(':');
        if !combined && parts.len() < 12 {
            continue;
        }
        let (txq, rxq, tr, tm, retrnsmt, uid, timeout, inode) = if combined {
            let (txq, rxq) = parts[4].split_once(':').unwrap_or((parts[4], "0"));
            let (tr, tm) = parts[5].split_once(':').unwrap_or((parts[5], "0"));
            (txq, rxq, tr, tm, parts[6], parts[7], parts[8], parts[9])
        } else {
            (parts[4], parts[5], parts[6], parts[7], parts[8], parts[9], parts[10], parts[11])
        };
        out.push(NetRow {
            sl: parts[0].trim_end_matches(':').to_string(),
            local_address: parts[1].to_string(),
            rem_address: parts[2].to_string(),
            st: parts[3].to_string(),
            tx_queue: txq.to_string(),
            rx_queue: rxq.to_string(),
            tr: tr.to_string(),
            tm_when: tm.to_string(),
            retrnsmt: retrnsmt.to_string(),
            uid: uid.to_string(),
            timeout: timeout.to_string(),
            inode: inode.to_string(),
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

/// Read and parse every table, skipping missing/unreadable files
/// (older kernels lack tcp6/udp).
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
        let mut rows = collect(&[
            ("/proc/net/tcp", "tcp"),
            ("/proc/net/tcp6", "tcp6"),
            ("/proc/net/udp", "udp"),
        ]);
        // TCP keeps LISTEN + ESTABLISHED only; UDP keeps everything.
        rows.retain(|r| r.family == "udp" || r.st == "0A" || r.st == "01");
        let cap = rows.len().min(MAX_ENTRIES);
        self.base.stats = Value::Array(rows.iter().take(cap).map(row_to_value).collect());
        Ok(())
    }
}
