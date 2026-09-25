//! Disk I/O plugin — per-device counters, bytes, and rates.
//!
//! One row per whole disk: completed reads/writes, byte totals, mean
//! latency per op, and tick-over-tick byte rates. Partitions and
//! virtual devices stay out (their counts live under the parent).

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::platform as plat;

pub const NAME: &str = "diskio";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(DiskioPlugin::new()));
}

/// One device row. Latencies are mean milliseconds per op (0.0 with
/// no ops, never NaN). Public so tests can build rows directly.
pub fn disk_to_value(d: &plat::linux::proc_diskstats::DiskStats) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("disk_name".into(), Value::String(d.name.clone()));
    obj.insert("read_count".into(), Value::Uint(d.reads_completed));
    obj.insert("write_count".into(), Value::Uint(d.writes_completed));
    obj.insert("read_bytes".into(), Value::Uint(plat::linux::proc_diskstats::read_bytes(d)));
    obj.insert("write_bytes".into(), Value::Uint(plat::linux::proc_diskstats::write_bytes(d)));
    obj.insert("read_latency_ms".into(), Value::Float(mean_ms(d.time_read_ms, d.reads_completed)));
    obj.insert("write_latency_ms".into(), Value::Float(mean_ms(d.time_write_ms, d.writes_completed)));
    Value::Object(obj)
}

fn mean_ms(total_ms: u64, ops: u64) -> f64 {
    if ops > 0 { total_ms as f64 / ops as f64 } else { 0.0 }
}

/// Whole-disk gate. The reader already drops ram/loop; partitions and
/// dm/md/zvol virtuals go here (their counts aggregate upward).
pub fn should_include(name: &str) -> bool {
    if is_partition(name) {
        return false;
    }
    if name.starts_with("dm-") || name.starts_with("zd") {
        return false;
    }
    // `md` plus a decimal index (any width — md100+ included).
    if let Some(rest) = name.strip_prefix("md")
        && !rest.is_empty()
        && rest.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
    true
}

/// Partition test, two layers: `/sys/block/<name>` existing means the
/// kernel registers a whole disk (authoritative — keeps sr0, zram0,
/// nbd0, mmcblk0, which names alone can't distinguish); otherwise the
/// name heuristic decides (covers synthetic names in tests).
pub fn is_partition(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if std::path::Path::new("/sys/block").join(name).exists() {
        return false;
    }
    is_partition_name(name)
}

/// Whole-disk families ending in a digit (their partitions, if any,
/// take a `p<N>` suffix instead).
const DIGIT_SUFFIXED_WHOLE_DISKS: &[&str] = &[
    "sr", "nbd", "zram", "rbd", "loop", "ram", "mmcblk", "fd",
    "mtdblock", "drbd", "dm-", "pmem", "ubi",
];

fn is_partition_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut cut = bytes.len();
    while cut > 0 && bytes[cut - 1].is_ascii_digit() {
        cut -= 1;
    }
    let stem = std::str::from_utf8(&bytes[..cut]).unwrap_or("");
    let has_trailing_digits = cut < bytes.len();
    // Denylisted stems are whole disks despite their digits.
    if DIGIT_SUFFIXED_WHOLE_DISKS.contains(&stem) {
        return false;
    }
    // s390 DASD (`dasda1`) and eMMC boot areas (`mmcblk0boot0`) carry
    // longer stems than the generic rule below allows.
    if has_trailing_digits
        && (stem.starts_with("dasd") || stem.starts_with("mmcblk") && stem.len() > "mmcblk".len()) {
            return true;
        }
    // NVMe-style `p<N>` suffix (nvme0n1p1, mmcblk0p1).
    if let Some(idx) = name.rfind('p')
        && idx > 0 {
            let tail = &name[idx + 1..];
            if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    // SCSI/virtio/IDE `<letters><digits>` with a short whole-disk
    // parent (sda1, vdb3 — but not nvme0n1, whose stem runs long).
    if !has_trailing_digits || stem.is_empty() || stem.len() < 2 || stem.len() > 4 {
        return false;
    }
    stem.bytes().any(|b| b.is_ascii_alphabetic())
}

pub struct DiskioPlugin {
    base: GlancesPluginModel,
    prev: std::collections::HashMap<String, (u64, u64)>,
    prev_at: Option<std::time::Instant>,
}

impl Default for DiskioPlugin {
    fn default() -> Self { Self::new() }
}

impl DiskioPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            prev: std::collections::HashMap::new(),
            prev_at: None,
        }
    }
}

impl Plugin for DiskioPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] {
        &["read_bytes_rate_per_sec", "write_bytes_rate_per_sec"]
    }
    fn get_key(&self) -> Option<&'static str> { Some("disk_name") }
    fn update(&mut self) -> Result<()> {
        let disks = plat::linux::proc_diskstats::read()?;
        let now = std::time::Instant::now();
        let dt = self.prev_at.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        let mut out = Vec::new();
        let mut cur = std::collections::HashMap::new();
        for d in &disks {
            if !should_include(&d.name) {
                continue;
            }
            let (r, w) = (
                plat::linux::proc_diskstats::read_bytes(d),
                plat::linux::proc_diskstats::write_bytes(d),
            );
            let (rr, wr) = match self.prev.get(&d.name) {
                Some((pr, pw)) if dt > 0.0 => (
                    r.saturating_sub(*pr) as f64 / dt,
                    w.saturating_sub(*pw) as f64 / dt,
                ),
                _ => (0.0, 0.0),
            };
            cur.insert(d.name.clone(), (r, w));
            let mut v = disk_to_value(d);
            if let Some(o) = v.as_object_mut() {
                o.insert("read_bytes_rate_per_sec".into(), Value::Float(rr.max(0.0)));
                o.insert("write_bytes_rate_per_sec".into(), Value::Float(wr.max(0.0)));
                o.insert("time_since_update".into(), Value::Float(dt.max(0.0)));
            }
            out.push(v);
        }
        self.base.stats = Value::Array(out);
        self.prev = cur;
        self.prev_at = Some(now);
        Ok(())
    }
    fn update_views(&mut self, events: &mut EventLog) {
        let Some(m) = self.model_mut() else { return };
        m.build_views(&[], Some("disk_name"), None);
        // Rates alert (cumulative counters would latch); each verdict
        // publishes on both its counter and its rate field.
        let stats = std::mem::replace(&mut m.stats, Value::Null);
        if let Value::Array(items) = &stats {
            for item in items {
                let Some(o) = item.as_object() else { continue };
                let Some(name) = o.get("disk_name").and_then(Value::as_str) else { continue };
                let rx = o.get("read_bytes_rate_per_sec").and_then(Value::as_f64).unwrap_or(0.0);
                let tx = o.get("write_bytes_rate_per_sec").and_then(Value::as_f64).unwrap_or(0.0);
                let rx_d = m.get_alert(rx, 0.0, 100.0, "rx", Some(name), false, true, None, Some(&mut *events));
                let tx_d = m.get_alert(tx, 0.0, 100.0, "tx", Some(name), false, true, None, Some(&mut *events));
                let entry = m.views.entry(name.to_string()).or_default();
                entry.insert("read_bytes".into(), rx_d.clone());
                entry.insert("read_bytes_rate_per_sec".into(), rx_d);
                entry.insert("write_bytes".into(), tx_d.clone());
                entry.insert("write_bytes_rate_per_sec".into(), tx_d);
            }
        }
        m.stats = stats;
    }
}
