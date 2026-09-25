//! Network plugin — per-NIC byte counters plus RX/TX rates.
//!
//! Two samples make a rate (first tick emits zeros). Every interface
//! with counters shows, loopback included — "connected" is decided
//! downstream from `is_up`, not by name here. Tunnel interfaces
//! (Tailscale, WireGuard) report operstate "unknown" while fully
//! working, so that counts as up.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "network";

/// Output cap — a host with hundreds of interfaces must not blow up
/// the JSON payload.
const MAX_NICS: usize = 64;

/// One NIC sample: name, rx/tx bytes, up flag, speed, addresses.
type NicSample = (String, u64, u64, bool, Option<u64>, Vec<String>);

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(NetworkPlugin::new()));
}

pub use super::net_role::{iface_is_up, iface_role};

pub struct NetworkPlugin {
    base: GlancesPluginModel,
    /// Previous tick's (rx, tx) per interface, for deltas.
    prev_counts: HashMap<String, (u64, u64)>,
    prev_time: Option<Instant>,
}

impl NetworkPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            prev_counts: HashMap::new(),
            prev_time: None,
        }
    }

    /// Shared row builder for the local and SNMP paths (same key
    /// contract; SNMP passes no addresses). Plain fields carry the
    /// tick-over-tick DELTA, `<field>_gauge` the cumulative counter,
    /// and `time_since_update` the window — delta/window is the live
    /// rate the widgets display.
    fn build_rows(&self, samples: &[NicSample], dt: f64) -> Vec<Value> {
        let mut out = Vec::with_capacity(samples.len().min(MAX_NICS));
        for (name, rx, tx, is_up, speed, ips) in samples {
            if out.len() >= MAX_NICS {
                break;
            }
            let (d_rx, d_tx) = match self.prev_counts.get(name) {
                Some((prx, ptx)) => (rx.saturating_sub(*prx) as f64, tx.saturating_sub(*ptx) as f64),
                None => (0.0, 0.0),
            };
            let (rx_r, tx_r) = if dt > 0.0 { (d_rx / dt, d_tx / dt) } else { (0.0, 0.0) };
            let mut obj = BTreeMap::new();
            obj.insert("key".into(), Value::String("interface_name".into()));
            obj.insert("interface_name".into(), Value::String(name.clone()));
            obj.insert("alias".into(), Value::Null);
            obj.insert("is_up".into(), Value::Bool(*is_up));
            obj.insert(
                "speed".into(),
                speed.map(Value::Uint).unwrap_or(Value::Null),
            );
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
            obj.insert(
                "ip_addresses".into(),
                Value::Array(ips.iter().map(|s| Value::String(s.clone())).collect()),
            );
            obj.insert("role".into(), Value::String(iface_role(name, ips).to_string()));
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
    fn history_items(&self) -> &[&'static str] {
        &["bytes_recv_rate_per_sec", "bytes_sent_rate_per_sec"]
    }
    fn get_key(&self) -> Option<&'static str> { Some("interface_name") }

    fn update_snmp(&mut self, ctx: &crate::core::snmp::SnmpCtx) -> Result<()> {
        // One ifEntry walk, columns grouped by instance suffix.
        let rows = ctx.client.walk("1.3.6.1.2.1.2.2.1", 4096)?;
        let mut cols: HashMap<(String, String), crate::core::snmp::SnmpValue> = HashMap::new();
        for (oid, v) in &rows {
            if let Some(rest) = oid.strip_prefix("1.3.6.1.2.1.2.2.1.")
                && let Some((col, idx)) = rest.split_once('.') {
                    cols.insert((col.to_string(), idx.to_string()), v.clone());
                }
        }
        let mut idxs: Vec<String> = cols.keys().map(|(_, i)| i.clone()).collect();
        idxs.sort();
        idxs.dedup();
        let num = |col: &str, idx: &str| {
            cols.get(&(col.to_string(), idx.to_string())).and_then(|v| v.as_f64()).unwrap_or(0.0)
        };
        let mut samples: Vec<NicSample> = Vec::new();
        for idx in &idxs {
            if num("3", idx) as u64 == 24 {
                continue; // softwareLoopback
            }
            let name = cols
                .get(&("2".to_string(), idx.to_string()))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            samples.push((
                name,
                num("10", idx) as u64,
                num("16", idx) as u64,
                num("8", idx) as u64 == 1,
                Some(num("5", idx) as u64),
                Vec::new(),
            ));
        }
        let now = Instant::now();
        let dt = self.prev_time.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        self.base.stats = Value::Array(self.build_rows(&samples, dt));
        self.prev_counts =
            samples.iter().map(|(n, rx, tx, _, _, _)| (n.clone(), (*rx, *tx))).collect();
        self.prev_time = Some(now);
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        let now = Instant::now();
        let dt = self.prev_time.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        let dev = plat::linux::proc_net_dev::read().unwrap_or_default();
        // Meta first: address attribution only considers interfaces
        // that are up.
        let mut infos = Vec::with_capacity(dev.len());
        for (name, s) in &dev {
            let meta = plat::linux::sys_class_net::read_meta(name).unwrap_or_default();
            infos.push((name.clone(), s.rx_bytes, s.tx_bytes, iface_is_up(&meta.operstate), meta.speed_mbps));
        }
        let ups: Vec<String> = infos.iter().filter(|i| i.3).map(|i| i.0.clone()).collect();
        let addrs =
            super::ip::attribute_ips(&ups, &super::ip::routes(), &super::ip::local_ips_from_fib_trie());
        let mut cur_counts: HashMap<String, (u64, u64)> = HashMap::with_capacity(dev.len());
        let mut samples = Vec::with_capacity(dev.len());
        for (name, rx, tx, is_up, speed_mbps) in &infos {
            cur_counts.insert(name.clone(), (*rx, *tx));
            samples.push((
                name.clone(),
                *rx,
                *tx,
                *is_up,
                speed_mbps.map(|v| v.saturating_mul(1_048_576)),
                addrs.get(name).cloned().unwrap_or_default(),
            ));
        }
        self.base.stats = Value::Array(self.build_rows(&samples, dt));
        self.prev_counts = cur_counts;
        self.prev_time = Some(now);
        Ok(())
    }
    fn update_views(&mut self, events: &mut EventLog) {
        let Some(m) = self.model_mut() else { return };
        m.build_views(&[], Some("interface_name"), None);
        // Per-interface rx/tx alerts on bit-rates; with no configured
        // thresholds the verdict falls back to link speed.
        let stats = std::mem::replace(&mut m.stats, Value::Null);
        if let Value::Array(items) = &stats {
            for item in items {
                let Some(o) = item.as_object() else { continue };
                let Some(name) = o.get("interface_name").and_then(Value::as_str) else { continue };
                // Aliased interfaces (eth0:1) alert under the real name.
                let real = name.split(':').next().unwrap_or(name).to_string();
                let rx = o.get("bytes_recv_rate_per_sec").and_then(Value::as_f64).unwrap_or(0.0);
                let tx = o.get("bytes_sent_rate_per_sec").and_then(Value::as_f64).unwrap_or(0.0);
                let speed = o.get("speed").and_then(Value::as_f64).unwrap_or(0.0);
                let mut rx_d =
                    m.get_alert(rx * 8.0, 0.0, 100.0, "rx", Some(&real), false, true, None, Some(&mut *events));
                let mut tx_d =
                    m.get_alert(tx * 8.0, 0.0, 100.0, "tx", Some(&real), false, true, None, Some(&mut *events));
                if rx_d == "DEFAULT" && speed > 0.0 {
                    rx_d = m.get_alert(rx * 8.0, 0.0, speed, "rx", None, false, false, None, Some(&mut *events));
                }
                if tx_d == "DEFAULT" && speed > 0.0 {
                    tx_d = m.get_alert(tx * 8.0, 0.0, speed, "tx", None, false, false, None, Some(&mut *events));
                }
                let entry = m.views.entry(name.to_string()).or_default();
                entry.insert("bytes_recv".into(), rx_d.clone());
                entry.insert("bytes_recv_rate_per_sec".into(), rx_d);
                entry.insert("bytes_sent".into(), tx_d.clone());
                entry.insert("bytes_sent_rate_per_sec".into(), tx_d);
            }
        }
        m.stats = stats;
    }
}
