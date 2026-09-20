//! Filesystem usage plugin — per-mount total/used/free/percent.
//!
//! Reads `/proc/mounts` to enumerate mount points, then calls
//! `statvfs(3)` on each. Filters out pseudo-fs types (tmpfs, devpts,
//! proc, sysfs, cgroup*) and mountpoint prefixes (/proc, /sys, /dev/pts,
//! /run, /dev, /var/run) that produce noise without useful info.
//!
//! Output is a `Value::Array` of `Value::Object`s, one per mount, with
//! key `mntpoint`.

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::{GlancesError, Result};
use crate::platform as plat;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "fs";

/// Filesystem types we ignore (pseudo / virtual).
const SKIP_FSTYPES: &[&str] = &[
    "tmpfs", "devpts", "proc", "sysfs", "cgroup", "cgroup2",
    "devtmpfs", "securityfs", "pstore", "efivarfs", "bpf",
    "autofs", "mqueue", "hugetlbfs", "fusectl", "configfs",
    "debugfs", "tracefs", "binfmt_misc", "ramfs",
];

/// Mountpoint prefixes we ignore.
const SKIP_MNT_PREFIXES: &[&str] = &[
    "/proc", "/sys", "/dev/pts", "/run", "/var/run",
];

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(FsPlugin::new()));
}

/// Parsed /proc/mounts entry.
#[derive(Debug, Clone, PartialEq)]
pub struct MountEntry {
    pub device: String,
    pub mountpoint: String,
    pub fstype: String,
    pub options: String,
}

/// Parse one line of `/proc/mounts`.
/// Format: `device mountpoint fstype options dump pass`
pub fn parse_mounts_line(line: &str) -> Option<MountEntry> {
    let mut parts = line.split_whitespace();
    let device = unescape_octal(parts.next()?);
    let mountpoint = unescape_octal(parts.next()?);
    let fstype = parts.next()?.to_string();
    // Options are a single comma-separated field; dump/pass follow.
    let options = parts.next().unwrap_or("").to_string();
    Some(MountEntry { device, mountpoint, fstype, options })
}

/// Decode `/proc/mounts` octal escapes: space→\040, tab→\011,
/// newline→\012, backslash→\134. Mountpoints containing spaces would
/// otherwise fail `statvfs` and be silently dropped.
fn unescape_octal(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len()
            && b[i + 1].is_ascii_digit() && b[i + 2].is_ascii_digit() && b[i + 3].is_ascii_digit()
        {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Should this mount be filtered out?
pub fn should_skip(entry: &MountEntry) -> bool {
    if SKIP_FSTYPES.contains(&entry.fstype.as_str()) {
        return true;
    }
    for prefix in SKIP_MNT_PREFIXES {
        // Path-boundary match: "/sys" and "/sys/..." go, but NOT
        // "/sysbackup" — a bare `starts_with` would over-exclude.
        if entry.mountpoint == *prefix
            || entry.mountpoint.starts_with(&format!("{}/", prefix))
        {
            return true;
        }
    }
    false
}

pub struct FsPlugin { base: GlancesPluginModel }

impl FsPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

impl Plugin for FsPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["percent"] }
    fn get_key(&self) -> Option<&'static str> { Some("mnt_point") }
    fn update_snmp(&mut self, ctx: &crate::core::snmp::SnmpCtx) -> Result<()> {
        // Default: UCD dskTable walk (KB units). Windows/ESXi: the
        // hrStorage walk with alloc_unit math (upstream parity).
        let use_hr = matches!(ctx.system_name.as_deref(), Some("windows") | Some("esxi"));
        let rows = ctx.client.walk(
            if use_hr { "1.3.6.1.2.1.25.2.3.1" } else { "1.3.6.1.4.1.2021.9.1" },
            4096,
        )?;
        let mut table: std::collections::HashMap<(String, String), String> = std::collections::HashMap::new();
        for (oid, v) in &rows {
            let base = if use_hr { "1.3.6.1.2.1.25.2.3.1." } else { "1.3.6.1.4.1.2021.9.1." };
            let k = oid.strip_prefix(base).and_then(|r| r.split_once('.'));
            if let Some((c, i)) = k {
                let key = (c.to_string(), i.to_string());
                if let Some(s) = v.as_str() {
                    table.insert(key, s.to_string());
                } else if let Some(n) = v.as_f64() {
                    table.insert(key, (n as u64).to_string());
                }
            }
        }
        let mut idxs: Vec<String> = table.keys().map(|(_, i)| i.clone()).collect();
        idxs.sort(); idxs.dedup();
        let get = |c: &str, i: &str| {
            table.get(&(c.to_string(), i.to_string())).cloned().unwrap_or_default()
        };
        let mut out = Vec::new();
        for idx in &idxs {
            let (mnt, dev, size, used, percent) = if use_hr {
                let alloc = get("4", idx).parse::<u64>().unwrap_or(0);
                let units = get("5", idx).parse::<u64>().unwrap_or(0);
                let used_u = get("6", idx).parse::<u64>().unwrap_or(0);
                let total = alloc.saturating_mul(units);
                let used_b = used_u.saturating_mul(alloc);
                let pct = if total > 0 { used_b as f64 / total as f64 * 100.0 } else { 0.0 };
                (get("3", idx), String::new(), total, used_b, pct)
            } else {
                let total = get("6", idx).parse::<f64>().unwrap_or(0.0) * 1024.0;
                let used = get("8", idx).parse::<f64>().unwrap_or(0.0) * 1024.0;
                let pct = get("9", idx).parse::<f64>().unwrap_or(0.0);
                (get("2", idx), get("3", idx), total as u64, used as u64, pct)
            };
            if mnt.is_empty() || size == 0 { continue; }
            let mut obj = std::collections::BTreeMap::new();
            obj.insert("mnt_point".into(), Value::String(mnt));
            obj.insert("device_name".into(), Value::String(dev));
            obj.insert("fs_type".into(), Value::String(String::new()));
            obj.insert("options".into(), Value::String(String::new()));
            obj.insert("size".into(), Value::Uint(size));
            obj.insert("used".into(), Value::Uint(used));
            obj.insert("free".into(), Value::Uint(size.saturating_sub(used)));
            obj.insert("percent".into(), Value::Float(percent));
            out.push(Value::Object(obj));
        }
        self.base.stats = Value::Array(out);
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        let text = fs::read_to_string("/proc/mounts").map_err(GlancesError::Io)?;
        let mounts = parse_mounts(&text);
        let mut out = Vec::new();
        for m in mounts {
            if should_skip(&m) { continue; }
            let usage = match plat::linux::statvfs::statvfs_path(&m.mountpoint) {
                Ok(u) => u,
                Err(_) => continue, // vanished mount — skip silently
            };
            // Upstream key contract (fs/__init__.py): device_name,
            // fs_type, mnt_point, options (mount opts string; also gates
            // the read-only-mount alert exemption), size/used/free/percent.
            let mut obj = BTreeMap::new();
            obj.insert("mnt_point".into(), Value::String(m.mountpoint.replace('\u{a0}', " ")));
            obj.insert("device_name".into(), Value::String(m.device));
            obj.insert("fs_type".into(), Value::String(m.fstype));
            obj.insert("options".into(), Value::String(m.options));
            obj.insert("size".into(), Value::Uint(usage.total));
            obj.insert("used".into(), Value::Uint(usage.used));
            obj.insert("free".into(), Value::Uint(usage.free));
            obj.insert("percent".into(), Value::Float(usage.percent));
            out.push(Value::Object(obj));
        }
        self.base.stats = Value::Array(out);
        Ok(())
    }
    fn update_views(&mut self, events: &mut EventLog) {
        if let Some(m) = self.model_mut() {
            m.build_views(&[], Some("mnt_point"), None);
            // Upstream fs update_views: per-mount `used` alert on
            // (size - free) / size, keyed by mount point — except
            // read-only mounts (#3143), which keep DEFAULT.
            // Move the array aside (no clone): `get_alert` needs
            // `&mut`, so stats can't stay borrowed across the call.
            let stats = std::mem::replace(&mut m.stats, Value::Null);
            if let Value::Array(items) = &stats {
                for item in items {
                    let Some(o) = item.as_object() else { continue; };
                    let name = match o.get("mnt_point").and_then(Value::as_str) {
                        Some(s) => s.to_string(),
                        None => continue,
                    };
                    let ro = o
                        .get("options")
                        .and_then(Value::as_str)
                        .map(|opts| opts.split(',').any(|f| f.trim() == "ro"))
                        .unwrap_or(false);
                    if ro {
                        continue;
                    }
                    let (size, free) = (
                        o.get("size").and_then(Value::as_f64).unwrap_or(0.0),
                        o.get("free").and_then(Value::as_f64).unwrap_or(0.0),
                    );
                    if size <= 0.0 {
                        continue;
                    }
                    let d = m.get_alert(size - free, 0.0, size, &name, None, false, false, None, Some(&mut *events));
                    m.views
                        .entry(name)
                        .or_default()
                        .insert("used".into(), d);
                }
            }
            m.stats = stats;
        }
    }
}

/// Parse all lines of `/proc/mounts`. Skips blank lines and lines that
/// don't parse (e.g. comment lines, though `/proc/mounts` has none).
pub fn parse_mounts(text: &str) -> Vec<MountEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        if let Some(e) = parse_mounts_line(line) {
            out.push(e);
        }
    }
    out
}