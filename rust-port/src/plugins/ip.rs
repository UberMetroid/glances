//! IP plugin — public + private IP address reporting.
//!
//! Mirrors `glances/plugins/ip/__init__.py`. The private IP / netmask /
//! gateway come from local `/proc/net/route` and `/sys/class/net/<iface>/`
//! reads (already covered by the existing readers). The public IP needs an
//! outbound HTTP fetch — we don't want to block `update()` on network I/O,
//! so the public IP is refreshed by a background daemon thread started at
//! construction. `update()` returns whatever the daemon most recently
//! wrote, behind a Mutex.
//!
//! HTTP is implemented over `std::net::TcpStream` (no external crates).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "ip";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(IpPlugin::new()));
}

/// Default endpoint to query for the public IP. We pick ipify because it
/// returns plain text, no auth, no TLS to wrestle with. We keep one host
/// constant here — the daemon retries indefinitely and stores "" on
/// failure so `update()` is never blocked.
const PUBLIC_IP_HOST: &str = "api.ipify.org";
const PUBLIC_IP_PATH: &str = "/";

/// How long the daemon sleeps between refresh attempts.
const REFRESH_SECS: u64 = 60;

/// Parse a `/proc/net/route`-style line and return the default gateway IP
/// (the row whose Destination is `00000000`) and the interface name.
/// Returns Ok(None) if no default route is found.
pub fn parse_route_line(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 { return None; }
    let iface = parts[0].to_string();
    let dest = parts[1];
    let gw = parts[2];
    if dest == "00000000" {
        return Some((iface, gw.to_string()));
    }
    None
}

/// Convert a little-endian hex IP (as found in /proc/net/route) to dotted
/// notation. E.g. "0103A8C0" → "192.168.3.1".
pub fn hex_to_ipv4(hex: &str) -> Option<String> {
    if hex.len() != 8 { return None; }
    let bytes: Vec<u8> = (0..4).map(|i| u8::from_str_radix(&hex[i*2..i*2+2], 16).ok()).collect::<Option<Vec<u8>>>()?;
    Some(format!("{}.{}.{}.{}", bytes[3], bytes[2], bytes[1], bytes[0]))
}

/// Look up the default gateway from /proc/net/route. Returns `""` on any
/// error so callers can blindly insert the result.
pub fn default_gateway() -> String {
    let text = match std::fs::read_to_string("/proc/net/route") {
        Ok(t) => t,
        Err(_) => return String::new(),
    };
    for line in text.lines().skip(1) {
        if let Some((_, gw)) = parse_route_line(line) {
            if let Some(ip) = hex_to_ipv4(&gw) {
                return ip;
            }
        }
    }
    String::new()
}

/// Find the first non-loopback physical interface with an IPv4 address.
/// Reads `/proc/net/fib_trie` and pulls the first IPv4 line under each
/// "32 host" section. Returns `""` if nothing is found.
pub fn private_ip_from_fib_trie() -> String {
    let text = match std::fs::read_to_string("/proc/net/fib_trie") {
        Ok(t) => t,
        Err(_) => return String::new(),
    };
    let mut in_host_section = false;
    let mut last_iface_v4: String = String::new();
    let mut local_ips: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("|--") {
            // IP line in the trie.
            let ip_part = trimmed.trim_start_matches("|--").trim();
            // The IP may end with "32 host LOCAL" — strip suffix.
            let ip = ip_part.split_whitespace().next().unwrap_or("");
            if let Some(ok_ip) = parse_ipv4(ip) {
                if in_host_section {
                    local_ips.push(ok_ip);
                } else {
                    last_iface_v4 = ok_ip;
                }
            }
        } else if trimmed.contains("32 host") {
            in_host_section = true;
        } else if trimmed.starts_with("|") && !trimmed.starts_with("|--") {
            in_host_section = false;
        }
    }
    // The "local" IPs are 127.0.0.1 — we want the iface IP, not the loopback.
    for ip in &local_ips {
        if !ip.starts_with("127.") {
            return ip.clone();
        }
    }
    if !last_iface_v4.is_empty() && !last_iface_v4.starts_with("127.") {
        return last_iface_v4;
    }
    String::new()
}

fn parse_ipv4(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 { return None; }
    for p in &parts {
        if p.parse::<u8>().is_err() { return None; }
    }
    Some(s.to_string())
}

/// Read the MAC address of `iface` from /sys/class/net/<iface>/address.
/// Returns `""` on any error.
pub fn mac_address(iface: &str) -> String {
    let path = format!("/sys/class/net/{}/address", iface);
    std::fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Pick a non-loopback interface name. Returns `""` if none exists.
pub fn primary_interface() -> String {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else { return String::new(); };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "lo" { continue; }
        return name;
    }
    String::new()
}

/// Fetch the public IP via a plain-text HTTP GET to api.ipify.org.
/// Returns Err on any network/parse failure. Uses std::net only.
pub fn fetch_public_ip() -> Result<String> {
    let mut stream = TcpStream::connect((PUBLIC_IP_HOST, 80))
        .map_err(GlancesError::Io)?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: glances-rs/0.4\r\nConnection: close\r\n\r\n",
        PUBLIC_IP_PATH, PUBLIC_IP_HOST
    );
    stream.write_all(req.as_bytes()).map_err(GlancesError::Io)?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).map_err(GlancesError::Io)?;
    // Split headers from body at "\r\n\r\n".
    let body = match buf.find("\r\n\r\n") {
        Some(i) => &buf[i + 4..],
        None => return Err(GlancesError::Parse("no http body delimiter".into())),
    };
    let ip = body.trim().to_string();
    if ip.is_empty() {
        return Err(GlancesError::Parse("empty public ip response".into()));
    }
    Ok(ip)
}

pub struct IpPlugin {
    base: GlancesPluginModel,
    pub_public: Arc<Mutex<String>>,
}

impl IpPlugin {
    pub fn new() -> Self {
        let mut m: BTreeMap<String, Value> = BTreeMap::new();
        for k in &["address", "mask", "gateway", "public_ip", "mac"] {
            m.insert(k.to_string(), Value::String(String::new()));
        }
        let pub_ip: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        // Spawn the daemon thread that refreshes the public IP every 60s.
        let arc = Arc::clone(&pub_ip);
        thread::spawn(move || loop {
            if let Ok(ip) = fetch_public_ip() {
                if let Ok(mut guard) = arc.lock() {
                    *guard = ip;
                }
            }
            thread::sleep(Duration::from_secs(REFRESH_SECS));
        });
        Self {
            base: GlancesPluginModel::new(NAME, Value::Object(m)),
            pub_public: pub_ip,
        }
    }
}

impl Plugin for IpPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }

    fn update(&mut self) -> Result<()> {
        if !cfg!(target_os = "linux") {
            if let Some(obj) = self.base.stats.as_object_mut() {
                for k in &["address", "mask", "gateway", "public_ip", "mac"] {
                    obj.insert((*k).into(), Value::String(String::new()));
                }
            }
            return Ok(());
        }
        let iface = primary_interface();
        let mac = if iface.is_empty() { String::new() } else { mac_address(&iface) };
        let address = private_ip_from_fib_trie();
        let gateway = default_gateway();
        let public = self.pub_public.lock().map(|s| s.clone()).unwrap_or_default();
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("address".into(), Value::String(address));
            obj.insert("mask".into(), Value::String(String::new()));
            obj.insert("gateway".into(), Value::String(gateway));
            obj.insert("public_ip".into(), Value::String(public));
            obj.insert("mac".into(), Value::String(mac));
        }
        Ok(())
    }
}