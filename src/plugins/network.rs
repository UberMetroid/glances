//! Network plugin — per-NIC byte counters + computed RX/TX rates.
//!
//! Mirrors `glances/plugins/network/__init__.py`. We need two reads to
//! produce a per-second rate: this tick and the previous tick. The first
//! update emits zero rates and the current gauge values.
//!
//! Shows every interface with counters, loopback included —
//! "connected" is decided downstream by `is_up`, not by name here.
//! Tunnel interfaces (tailscale, wireguard) report operstate
//! "unknown" while fully working, so that counts as up.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "network";

/// Maximum entries in the output array — safety cap so a host with
/// hundreds of interfaces doesn't blow up the JSON payload.
const MAX_NICS: usize = 64;

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(NetworkPlugin::new()));
}

/// Operstate → connected. Tunnels say "unknown" while working.
pub fn iface_is_up(operstate: &str) -> bool {
    matches!(operstate, "up" | "unknown")
}

pub struct NetworkPlugin {
    base: GlancesPluginModel,
    /// `(rx_bytes, tx_bytes)` from the previous tick, keyed by iface name.
    prev_counts: HashMap<String, (u64, u64)>,
    /// Wall time at the previous tick.
    prev_time: Option<Instant>,
}

impl NetworkPlugin {
    pub fn new() -> Self {
        let stats_init = Value::Array(Vec::new());
        Self {
            base: GlancesPluginModel::new(NAME, stats_init),
            prev_counts: HashMap::new(),
            prev_time: None,
        }
    }

    /// Shared row builder: `(name, rx, tx, is_up, speed_bps, ips)`
    /// samples plus tick-over-tick rates from `prev_counts`. Used by
    /// the local and SNMP paths so both emit the same key contract
    /// (SNMP passes no addresses).
    fn build_rows(&self, samples: &[(String, u64, u64, bool, Option<u64>, Vec<String>)], dt: f64) -> Vec<Value> {
        let mut out = Vec::with_capacity(samples.len().min(MAX_NICS));
        for (name, rx, tx, is_up, speed, ips) in samples {
            if out.len() >= MAX_NICS { break; }
            // Upstream `_manage_rate` parity: plain fields carry the
            // tick-over-tick DELTA, `<field>_gauge` the cumulative
            // counter, `time_since_update` the window seconds, so
            // delta/window is the live rate the widgets display.
            let (d_rx, d_tx) = match self.prev_counts.get(name) {
                Some((prx, ptx)) => (
                    rx.saturating_sub(*prx) as f64,
                    tx.saturating_sub(*ptx) as f64,
                ),
                None => (0.0, 0.0),
            };
            let (rx_r, tx_r) = if dt > 0.0 { (d_rx / dt, d_tx / dt) } else { (0.0, 0.0) };
            let mut obj = BTreeMap::new();
            obj.insert("key".into(), Value::String("interface_name".into()));
            obj.insert("interface_name".into(), Value::String(name.clone()));
            obj.insert("alias".into(), Value::Null);
            obj.insert("is_up".into(), Value::Bool(*is_up));
            obj.insert("speed".into(), match speed {
                Some(v) => Value::Uint(*v),
                None => Value::Null,
            });
            obj.insert("bytes_recv".into(), Value::Float(d_rx));
            obj.insert("bytes_recv_gauge".into(), Value::Float(*rx as f64));
            obj.insert("bytes_recv_rate_per_sec".into(), Value::Float(rx_r));
            obj.insert("bytes_sent".into(), Value::Float(d_tx));
            obj.insert("bytes_sent_gauge".into(), Value::Float(*tx as f64));
            obj.insert("bytes_sent_rate_per_sec".into(), Value::Float(tx_r));
            obj.insert("bytes_all".into(), Value::Float(d_rx + d_tx));
            obj.insert("bytes_all_gauge".into(), Value::Float(rx.saturating_add(*tx) as f64));
            obj.insert("bytes_all_rate_per_sec".into(), Value::Float(rx_r + tx_r));
            obj.insert("time_since_update".into(), Value::Float(dt.max(0.0)));
            obj.insert("ip_addresses".into(), Value::Array(
                ips.iter().map(|s| Value::String(s.clone())).collect()));
            out.push(Value::Object(obj));
        }
        out
    }
}

impl Default for NetworkPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for NetworkPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) {
        self.base.reset();
        self.prev_counts.clear();
        self.prev_time = None;
    }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["bytes_recv_rate_per_sec", "bytes_sent_rate_per_sec"] }
    fn get_key(&self) -> Option<&'static str> { Some("interface_name") }

    fn update_snmp(&mut self, ctx: &crate::core::snmp::SnmpCtx) -> Result<()> {
        // One walk over ifEntry; group columns by instance suffix.
        let rows = ctx.client.walk("1.3.6.1.2.1.2.2.1", 4096)?;
        let mut cols: HashMap<(String, String), crate::core::snmp::SnmpValue> = HashMap::new();
        for (oid, v) in &rows {
            if let Some(rest) = oid.strip_prefix("1.3.6.1.2.1.2.2.1.") {
                if let Some((col, idx)) = rest.split_once('.') {
                    cols.insert((col.to_string(), idx.to_string()), v.clone());
                }
            }
        }
        let mut idxs: Vec<String> = cols.keys().map(|(_, i)| i.clone()).collect();
        idxs.sort();
        idxs.dedup();
        let num = |col: &str, idx: &str| {
            cols.get(&(col.to_string(), idx.to_string())).and_then(|v| v.as_f64()).unwrap_or(0.0)
        };
        let name_of = |idx: &str| {
            cols.get(&("2".to_string(), idx.to_string()))
                .and_then(|v| v.as_str()).unwrap_or("").to_string()
        };
        let mut samples: Vec<(String, u64, u64, bool, Option<u64>, Vec<String>)> = Vec::new();
        for idx in &idxs {
            if num("3", idx) as u64 == 24 { continue; } // softwareLoopback
            let (rx, tx) = (num("10", idx) as u64, num("16", idx) as u64);
            samples.push((name_of(idx), rx, tx, num("8", idx) as u64 == 1, Some(num("5", idx) as u64), Vec::new()));
        }
        let now = Instant::now();
        let dt = self.prev_time.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        self.base.stats = Value::Array(self.build_rows(&samples, dt));
        let mut cur = HashMap::new();
        for (n, rx, tx, _, _, _) in &samples { cur.insert(n.clone(), (*rx, *tx)); }
        self.prev_counts = cur;
        self.prev_time = Some(now);
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        let now = Instant::now();
        let dt = self.prev_time
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);

        // Snapshot of current per-iface byte counters.
        let dev = plat::linux::proc_net_dev::read().unwrap_or_default();
        // Meta first: attribution's elimination step only considers
        // interfaces that are up.
        let mut infos = Vec::with_capacity(dev.len());
        for (name, s) in &dev {
            let meta = plat::linux::sys_class_net::read_meta(name).unwrap_or_default();
            infos.push((name.clone(), s.rx_bytes, s.tx_bytes,
                iface_is_up(&meta.operstate), meta.speed_mbps));
        }
        let ups: Vec<String> = infos.iter().filter(|i| i.3).map(|i| i.0.clone()).collect();
        let addrs = super::ip::attribute_ips(
            &ups, &super::ip::routes(), &super::ip::local_ips_from_fib_trie());
        let mut cur_counts: HashMap<String, (u64, u64)> = HashMap::with_capacity(dev.len());
        let mut samples = Vec::with_capacity(dev.len());
        for (name, rx, tx, is_up, speed_mbps) in &infos {
            cur_counts.insert(name.clone(), (*rx, *tx));
            let speed = speed_mbps.map(|v| v.saturating_mul(1_048_576));
            let ips = addrs.get(name).cloned().unwrap_or_default();
            samples.push((name.clone(), *rx, *tx, *is_up, speed, ips));
        }

        self.base.stats = Value::Array(self.build_rows(&samples, dt));
        self.prev_counts = cur_counts;
        self.prev_time = Some(now);
        Ok(())
    }
    fn update_views(&mut self, events: &mut EventLog) {
        if let Some(m) = self.model_mut() {
            m.build_views(&[], Some("interface_name"), None);
            // Upstream network update_views: per-interface rx/tx alerts
            // on bit-rates vs config thresholds, falling back to the
            // interface speed when unset.
            // Move the array aside (no clone): alert calls need `&mut`.
            let stats = std::mem::replace(&mut m.stats, Value::Null);
            if let Value::Array(items) = &stats {
            for item in items {
                let o = match item.as_object() {
                    Some(o) => o,
                    None => continue,
                };
                let name = match o.get("interface_name").and_then(Value::as_str) {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let real = name.split(':').next().unwrap_or(&name).to_string();
                let rx = o.get("bytes_recv_rate_per_sec").and_then(Value::as_f64).unwrap_or(0.0);
                let tx = o.get("bytes_sent_rate_per_sec").and_then(Value::as_f64).unwrap_or(0.0);
                let speed = o.get("speed").and_then(Value::as_f64).unwrap_or(0.0);
                let mut rx_d = m.get_alert(rx * 8.0, 0.0, 100.0, "rx", Some(&real), false, true, None, Some(&mut *events));
                let mut tx_d = m.get_alert(tx * 8.0, 0.0, 100.0, "tx", Some(&real), false, true, None, Some(&mut *events));
                // No configured thresholds → compare against link speed.
                if rx_d == "DEFAULT" && speed > 0.0 {
                    rx_d = m.get_alert(rx * 8.0, 0.0, speed, "rx", None, false, false, None, Some(&mut *events));
                }
                if tx_d == "DEFAULT" && speed > 0.0 {
                    tx_d = m.get_alert(tx * 8.0, 0.0, speed, "tx", None, false, false, None, Some(&mut *events));
                }
                let entry = m.views.entry(name).or_default();
                entry.insert("bytes_recv".into(), rx_d.clone());
                entry.insert("bytes_recv_rate_per_sec".into(), rx_d);
                entry.insert("bytes_sent".into(), tx_d.clone());
                entry.insert("bytes_sent_rate_per_sec".into(), tx_d);
            }
            }
            m.stats = stats;
        }
    }
}