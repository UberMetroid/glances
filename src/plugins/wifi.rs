//! Wi-Fi plugin — link quality and signal from /proc/net/wireless.
//!
//! /proc/net/wireless format (Linux):
//!   Inter-| sta-|   Quality        |   Discarded packets ...
//!    face | tus | link level noise |  nwid  crypt   frag ...
//!   wlan0: 0000   70.  -40.  -95.   0    0    0     0 ...
//!
//! `link` is the link quality (0-100), `level` and `noise` are in dBm.
//! `status` is a bitmask; bit 0x1 means NIC is connected to an AP.
//!
//! SSID cannot be obtained without nl80211 (we can't link against
//! libnl); we expose it as the placeholder "?" so the field exists.
//! Real SSID lookup belongs to a separate plugin that links against
//! libiw / nl80211 in a later milestone.

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "wifi";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(WifiPlugin::new()));
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct WifiLink {
    pub interface: String,
    pub ssid: String,         // always "?" in this M9 — placeholder
    pub signal_dbm: Option<f64>,
    pub bitrate_mbps: Option<f64>,
    pub link_quality_pct: Option<f64>,
}

/// Parse one row of /proc/net/wireless. The trailing fields after
/// `noise` are discarded packets + misc + WE version — we don't use
/// them. Returns None for the header rows.
pub fn parse_line(line: &str) -> Option<WifiLink> {
    let colon = line.find(':')?;
    let iface = line[..colon].trim().to_string();
    let rest = line[colon + 1..].trim();

    // The first field is the status bitmask (e.g. "0000"). After it
    // come three dot-terminated numbers: link quality, level, noise.
    let mut tokens = rest.split_whitespace();
    let status = tokens.next()?;
    // Reject header lines — they have a non-numeric first token.
    if status.chars().any(|c| !c.is_ascii_hexdigit()) {
        return None;
    }

    // Find the first three "."-terminated tokens (link, level, noise).
    let mut dotted: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if tok.ends_with('.') { dotted.push(tok); }
        if dotted.len() == 3 { break; }
    }
    if dotted.len() < 3 { return None; }

    let parse_dotted = |s: &str| -> Option<f64> {
        s.trim_end_matches('.').parse::<f64>().ok()
    };
    let link_quality_pct = parse_dotted(dotted[0]);
    let signal_dbm = parse_dotted(dotted[1]);
    let _noise_dbm = parse_dotted(dotted[2]);

    // Bit 0x1 of status == associated.
    let _associated = status != "0000";

    Some(WifiLink {
        interface: iface,
        ssid: "?".to_string(),
        signal_dbm,
        bitrate_mbps: None, // not present in /proc/net/wireless
        link_quality_pct,
    })
}

pub fn parse(text: &str) -> Vec<WifiLink> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(w) = parse_line(line) { out.push(w); }
    }
    out
}

/// Best-effort link speed from sysfs — exposed so future revisions
/// can wire it in. /sys/class/net/<iface>/wireless/ doesn't exist;
/// speed comes from `<iface>/speed` (Mbps).
fn read_speed_mbps(iface: &str) -> Option<f64> {
    let s = fs::read_to_string(format!("/sys/class/net/{}/speed", iface)).ok()?;
    let n: u64 = s.trim().parse().ok()?;
    if n == 0 || n == u64::MAX / 2 { return None; }
    Some(n as f64)
}

pub fn wifi_to_value(w: &WifiLink) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("interface".into(), Value::String(w.interface.clone()));
    obj.insert("ssid".into(), Value::String(w.ssid.clone()));
    obj.insert("signal_dbm".into(), match w.signal_dbm {
        Some(v) => Value::Float(v),
        None => Value::Null,
    });
    obj.insert("bitrate_mbps".into(), match w.bitrate_mbps {
        Some(v) => Value::Float(v),
        None => Value::Null,
    });
    obj.insert("link_quality_pct".into(), match w.link_quality_pct {
        Some(v) => Value::Float(v),
        None => Value::Null,
    });
    Value::Object(obj)
}

pub struct WifiPlugin { base: GlancesPluginModel }

impl WifiPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

impl Default for WifiPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for WifiPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn get_key(&self) -> Option<&'static str> { Some("interface") }

    fn update(&mut self) -> Result<()> {
        // /proc/net/wireless is missing on hosts without wireless NICs;
        // treat that as an empty array rather than an error.
        let text = match fs::read_to_string("/proc/net/wireless") {
            Ok(t) => t,
            Err(_) => {
                self.base.stats = Value::Array(Vec::new());
                return Ok(());
            }
        };
        let links = parse(&text);
        let out: Vec<Value> = links.iter().map(|w| {
            let mut v = wifi_to_value(w);
            // Fill in bitrate from sysfs when available.
            if let Some(mbps) = read_speed_mbps(&w.interface)
                && let Some(obj) = v.as_object_mut() {
                    obj.insert("bitrate_mbps".into(), Value::Float(mbps));
                }
            v
        }).collect();
        self.base.stats = Value::Array(out);
        Ok(())
    }
}
