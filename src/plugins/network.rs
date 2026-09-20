//! Network plugin — per-NIC byte counters + computed RX/TX rates.
//!
//! Mirrors `glances/plugins/network/__init__.py`. We need two reads to
//! produce a per-second rate: this tick and the previous tick. The first
//! update emits zero rates and the current gauge values.
//!
//! Filters out the loopback interface (`lo`) by default; the spec calls
//! for exposing it via `Args` later.

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
    fn get_key(&self) -> Option<&'static str> { Some("interface_name") }

    fn update(&mut self) -> Result<()> {
        let now = Instant::now();
        let dt = self.prev_time
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);

        // Snapshot of current per-iface byte counters.
        let dev = plat::linux::proc_net_dev::read().unwrap_or_default();
        let mut cur_counts: HashMap<String, (u64, u64)> = HashMap::with_capacity(dev.len());
        for (name, s) in &dev {
            cur_counts.insert(name.clone(), (s.rx_bytes, s.tx_bytes));
        }

        let mut out: Vec<Value> = Vec::with_capacity(dev.len().min(MAX_NICS));
        for (name, s) in &dev {
            if name == "lo" { continue; }
            if out.len() >= MAX_NICS { break; }

            // Per-NIC meta from /sys/class/net/<name>; never fatal.
            let meta = plat::linux::sys_class_net::read_meta(name).unwrap_or_default();
            let is_up = meta.operstate == "up";

            let (rx_g, tx_g) = (s.rx_bytes as f64, s.tx_bytes as f64);
            let (rx_r, tx_r) = if dt > 0.0 {
                match self.prev_counts.get(name) {
                    Some((prx, ptx)) => {
                        let drx = s.rx_bytes.saturating_sub(*prx) as f64 / dt;
                        let dtx = s.tx_bytes.saturating_sub(*ptx) as f64 / dt;
                        (drx.max(0.0), dtx.max(0.0))
                    }
                    None => (0.0, 0.0),
                }
            } else {
                (0.0, 0.0)
            };

            // Upstream key contract: interface_name (real name), alias
            // (config alias, None when unset), byte counters + rate
            // siblings, combined bytes_all, speed in bits/sec.
            let mut obj = BTreeMap::new();
            obj.insert("interface_name".into(), Value::String(name.clone()));
            obj.insert("alias".into(), Value::Null);
            obj.insert("is_up".into(), Value::Bool(is_up));
            obj.insert("speed".into(), match meta.speed_mbps {
                Some(v) => Value::Uint(v.saturating_mul(1_048_576)),
                None => Value::Null,
            });
            obj.insert("bytes_recv".into(), Value::Float(rx_g));
            obj.insert("bytes_recv_rate_per_sec".into(), Value::Float(rx_r));
            obj.insert("bytes_sent".into(), Value::Float(tx_g));
            obj.insert("bytes_sent_rate_per_sec".into(), Value::Float(tx_r));
            obj.insert("bytes_all".into(), Value::Float(rx_g + tx_g));
            obj.insert("bytes_all_rate_per_sec".into(), Value::Float(rx_r + tx_r));
            out.push(Value::Object(obj));
        }

        self.base.stats = Value::Array(out);
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
            let items = match m.stats.clone() {
                Value::Array(items) => items,
                _ => return,
            };
            for item in &items {
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
    }
}