//! IP plugin — public + private IP address reporting.
//!
//! Mirrors `glances/plugins/ip/__init__.py`. The private IP / netmask /
//! gateway come from local `/proc/net/route` and `/sys/class/net/<iface>/`
//! reads (already covered by the existing readers). The public IP needs an
//! outbound HTTP fetch — we don't want to block `update()` on network I/O,
//! so a single process-wide daemon thread refreshes it (see `public_ip`);
//! `update()` returns whatever it most recently wrote.
//!
//! `address`/`mask`/`gateway`/`mac` all describe the interface holding
//! the default route — paired via `route.rs` so multi-homed hosts can't
//! mix fields from different interfaces.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

mod attribute;
mod public_ip;
mod route;
pub use attribute::attribute_ips;
pub use public_ip::{configure as configure_public, extract_ip, fetch_public_ip, resolve_api_url, PublicCfg, PUBLIC_API_ENV};
pub use route::{address_for_iface, default_gateway, default_iface, hex_to_ipv4,
    ipv4_to_u32, local_ips_from_fib_trie, mask_from_route, parse_fib_trie,
    parse_route_line, routes, RouteRow};

pub const NAME: &str = "ip";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    // The one-per-process daemon is started here — the production entry
    // point — not in `new()`, so tests and ad-hoc constructions never
    // spawn a thread or fire outbound HTTP.
    public_ip::spawn_daemon();
    stats.register(Box::new(IpPlugin::new()));
}

/// First non-loopback local IPv4, or `""`.
pub fn private_ip_from_fib_trie() -> String {
    local_ips_from_fib_trie()
        .into_iter()
        .find(|ip| !ip.starts_with("127."))
        .unwrap_or_default()
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

pub struct IpPlugin {
    base: GlancesPluginModel,
    pub_public: Arc<Mutex<String>>,
}

impl Default for IpPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl IpPlugin {
    pub fn new() -> Self {
        let mut m: BTreeMap<String, Value> = BTreeMap::new();
        for k in &["address", "mask", "gateway", "public_ip", "mac"] {
            m.insert(k.to_string(), Value::String(String::new()));
        }
        Self {
            base: GlancesPluginModel::new(NAME, Value::Object(m)),
            // Shares the process-wide cell; nothing spawns here.
            pub_public: public_ip::cell(),
        }
    }
}

impl Plugin for IpPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
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
        // Everything below describes ONE interface — the one holding
        // the default route — so address/mask/gateway/mac can't mix
        // values from different interfaces on multi-homed hosts.
        let iface = default_iface();
        let mac = if iface.is_empty() { String::new() } else { mac_address(&iface) };
        let mask = if iface.is_empty() { String::new() } else { mask_from_route(&iface) };
        let gateway = default_gateway();
        let local_ips = local_ips_from_fib_trie();
        let address = if iface.is_empty() {
            private_ip_from_fib_trie()
        } else {
            address_for_iface(&iface, &routes(), &local_ips)
        };
        let public = self.pub_public.lock().map(|s| s.clone()).unwrap_or_default();
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("address".into(), Value::String(address));
            obj.insert("mask".into(), Value::String(mask));
            obj.insert("gateway".into(), Value::String(gateway));
            obj.insert("public_ip".into(), Value::String(public));
            obj.insert("mac".into(), Value::String(mac));
        }
        Ok(())
    }
}
