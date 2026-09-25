//! Filesystem usage plugin — per-mount total/used/free/percent.
//!
//! Mounts enumerate from /proc/mounts with statvfs(3) per mount.
//! Pseudo-filesystem types and virtual prefixes filter out (noise
//! without useful info). In containers, `GLANCES_ROOTFS` redirects to
//! the host view (see `fs_rootfs`).

use crate::core::error::Result;
use crate::core::events::EventLog;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::plugins::fs_rootfs;

pub const NAME: &str = "fs";

/// Ignored filesystem types (pseudo / virtual).
const SKIP_FSTYPES: &[&str] = &[
    "tmpfs", "devpts", "proc", "sysfs", "cgroup", "cgroup2",
    "devtmpfs", "securityfs", "pstore", "efivarfs", "bpf",
    "autofs", "mqueue", "hugetlbfs", "fusectl", "configfs",
    "debugfs", "tracefs", "binfmt_misc", "ramfs",
];

/// Ignored mountpoint prefixes.
const SKIP_MNT_PREFIXES: &[&str] = &[
    "/proc", "/sys", "/dev/pts", "/run", "/var/run",
];

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(FsPlugin::new()));
}

/// One parsed /proc/mounts entry.
#[derive(Debug, Clone, PartialEq)]
pub struct MountEntry {
    pub device: String,
    pub mountpoint: String,
    pub fstype: String,
    pub options: String,
}

/// Parse a mounts line: `device mountpoint fstype options dump pass`.
/// Options are one comma-separated field; dump/pass never join it.
pub fn parse_mounts_line(line: &str) -> Option<MountEntry> {
    let mut parts = line.split_whitespace();
    let entry = MountEntry {
        device: unescape_octal(parts.next()?),
        mountpoint: unescape_octal(parts.next()?),
        fstype: parts.next()?.to_string(),
        options: parts.next().unwrap_or("").to_string(),
    };
    Some(entry)
}

/// Decode mounts octal escapes (space→\040, tab→\011, newline→\012,
/// backslash→\134). Paths with spaces would otherwise fail statvfs
/// and drop silently.
fn unescape_octal(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let esc = bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4].iter().all(|b| b.is_ascii_digit());
        if esc
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(v);
                i += 4;
                continue;
            }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a mount filters out: pseudo type, or a path-boundary
/// prefix hit (`/sys` and `/sys/...` go; `/sysbackup` stays).
pub fn should_skip(entry: &MountEntry) -> bool {
    if SKIP_FSTYPES.contains(&entry.fstype.as_str()) {
        return true;
    }
    SKIP_MNT_PREFIXES.iter().any(|p| entry.mountpoint == *p || entry.mountpoint.starts_with(&format!("{p}/")))
}

pub struct FsPlugin { base: GlancesPluginModel }

impl Default for FsPlugin {
    fn default() -> Self { Self::new() }
}

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
        // UCD dskTable (KB units) by default; windows/esxi walk
        // hrStorage with alloc-unit math instead.
        let use_hr = matches!(ctx.system_name.as_deref(), Some("windows") | Some("esxi"));
        let base = if use_hr { "1.3.6.1.2.1.25.2.3.1" } else { "1.3.6.1.4.1.2021.9.1" };
        let rows = ctx.client.walk(base, 4096)?;
        let mut table: std::collections::HashMap<(String, String), String> = std::collections::HashMap::new();
        for (oid, v) in &rows {
            let cols = oid.strip_prefix(&format!("{base}.")).and_then(|r| r.split_once('.'));
            if let Some((c, i)) = cols {
                let rendered = v
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_f64().map(|n| (n as u64).to_string()));
                if let Some(s) = rendered {
                    table.insert((c.to_string(), i.to_string()), s);
                }
            }
        }
        let mut idxs: Vec<String> = table.keys().map(|(_, i)| i.clone()).collect();
        idxs.sort();
        idxs.dedup();
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
                (get("2", idx), get("3", idx), total as u64, used as u64, get("9", idx).parse::<f64>().unwrap_or(0.0))
            };
            if mnt.is_empty() || size == 0 {
                continue;
            }
            let mut obj = std::collections::BTreeMap::new();
            obj.insert("key".into(), Value::String("mnt_point".into()));
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
        // Rows carry device_name, fs_type, mnt_point, options (the
        // mount-opts string also gates the read-only alert exemption
        // below), size/used/free/percent.
        let root = fs_rootfs::resolve();
        self.base.stats = Value::Array(fs_rootfs::read_mounts_under(&root)?);
        Ok(())
    }
    fn update_views(&mut self, events: &mut EventLog) {
        let Some(m) = self.model_mut() else { return };
        m.build_views(&[], Some("mnt_point"), None);
        // Per-mount `used` alert on (size−free)/size — except read-only
        // mounts, which keep DEFAULT.
        let stats = std::mem::replace(&mut m.stats, Value::Null);
        if let Value::Array(items) = &stats {
            for item in items {
                let Some(o) = item.as_object() else { continue };
                let Some(name) = o.get("mnt_point").and_then(Value::as_str) else { continue };
                let read_only = o
                    .get("options")
                    .and_then(Value::as_str)
                    .map(|opts| opts.split(',').any(|f| f.trim() == "ro"))
                    .unwrap_or(false);
                if read_only {
                    continue;
                }
                let size = o.get("size").and_then(Value::as_f64).unwrap_or(0.0);
                let free = o.get("free").and_then(Value::as_f64).unwrap_or(0.0);
                if size <= 0.0 {
                    continue;
                }
                let d = m.get_alert(size - free, 0.0, size, name, None, false, false, None, Some(&mut *events));
                m.views.entry(name.to_string()).or_default().insert("used".into(), d);
            }
        }
        m.stats = stats;
    }
}

/// Parse a whole mounts table, skipping blanks and unparsable lines.
pub fn parse_mounts(text: &str) -> Vec<MountEntry> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(parse_mounts_line)
        .collect()
}
