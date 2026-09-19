//! Disk I/O plugin — per-device read/write counts and bytes.
//!
//! Wraps the existing `platform::linux::proc_diskstats::read()` reader and
//! formats each disk into a dict with key `disk_name`. Filters out
//! partitions, ram/loop devices (already done by the reader).
//!
//! Output is a `Value::Array` of `Value::Object`s.

use std::collections::BTreeMap;

use crate::core::error::Result;
use crate::platform as plat;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "diskio";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(DiskioPlugin::new()));
}

/// Build a `Value::Object` for one disk. Centralised so tests can
/// call it directly without spinning up the full plugin.
pub fn disk_to_value(d: &plat::linux::proc_diskstats::DiskStats) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("disk_name".into(), Value::String(d.name.clone()));
    obj.insert(
        "read_count".into(),
        Value::Uint(d.reads_completed),
    );
    obj.insert(
        "write_count".into(),
        Value::Uint(d.writes_completed),
    );
    obj.insert(
        "read_bytes".into(),
        Value::Uint(plat::linux::proc_diskstats::read_bytes(d)),
    );
    obj.insert(
        "write_bytes".into(),
        Value::Uint(plat::linux::proc_diskstats::write_bytes(d)),
    );
    Value::Object(obj)
}

/// Filter out devices we don't want to surface. The reader already drops
/// `ram*` and `loop*`; we add partitions and dm/md-style virtual devices
/// since their counts are aggregated under the parent device.
pub fn should_include(name: &str) -> bool {
    // Skip partitions (sda1, nvme0n1p1, vda2 etc.) — keep only whole disks.
    // Heuristic: partition entries are usually named `<parent><digit>`
    // OR `<parent>p<digit>` (nvme convention).
    if is_partition(name) { return false; }
    // Skip device-mapper, MD, and zero-size pseudo block devices.
    if name.starts_with("dm-") { return false; }
    if name.starts_with("md") && name.len() <= 4 { return false; }
    if name.starts_with("zd") { return false; }
    true
}

/// Naive partition heuristic: detects partitions by their canonical
/// Linux kernel naming.
///
/// - NVMe partitions: end with `p<N>` (e.g. `nvme0n1p1`). Whole-disk
///   NVMe namespaces end with `n<N>` (e.g. `nvme0n1`).
/// - SCSI / virtio / IDE partitions: short letter prefix + digits
///   (e.g. `sda1`, `vdb2`, `xvda3`). Whole disks are letter-only or
///   letter+digit (e.g. `sda`, `vdb`, `xvda`).
pub fn is_partition(name: &str) -> bool {
    if name.is_empty() { return false; }
    // NVMe-style: ends with 'p' followed by digits.
    // e.g. nvme0n1p1 → true; nvme0n1 → false.
    if let Some(idx) = name.rfind('p') {
        let tail = &name[idx + 1..];
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            // 'p' must not be at index 0 (no parent before it).
            // Also, the 'p' should not be the first character of the
            // name and the bytes before it should look like a device.
            if idx > 0 {
                return true;
            }
        }
    }
    // SCSI / virtio / IDE: trailing digit run after a parent whose name
    // is letters + digits, with a length typical of whole-disk names
    // (3–5 chars: sda, sdb, xvdb, vda, hda, ...). Length 5+ tends to
    // indicate a nested or NVMe-style name — we keep `nvme0n1` here.
    let bytes = name.as_bytes();
    let last = match bytes.last() {
        Some(b) => *b,
        None => return false,
    };
    if !last.is_ascii_digit() { return false; }
    // Walk back over trailing digits.
    let mut cut = bytes.len();
    while cut > 0 && bytes[cut - 1].is_ascii_digit() {
        cut -= 1;
    }
    let prefix = &bytes[..cut];
    // Parent must contain at least one letter AND look like a whole
    // disk (length 3–4 typically). Whole-disk names are short; longer
    // prefixes (nvme0n, mpath, ...) usually indicate the device
    // itself, not a partition parent.
    if prefix.is_empty() { return false; }
    let prefix_has_letter = prefix.iter().any(|b| b.is_ascii_alphabetic());
    if !prefix_has_letter { return false; }
    // Length 2-4 → typical whole-disk names (sda, sdb, vdb, hda).
    // nvme0n (length 5) is the namespace parent, not a partition
    // parent.
    if prefix.len() < 2 || prefix.len() > 4 { return false; }
    true
}

pub struct DiskioPlugin { base: GlancesPluginModel }

impl DiskioPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

impl Plugin for DiskioPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        let disks = plat::linux::proc_diskstats::read()?;
        let mut out = Vec::new();
        for d in &disks {
            if !should_include(&d.name) { continue; }
            out.push(disk_to_value(d));
        }
        self.base.stats = Value::Array(out);
        Ok(())
    }
}