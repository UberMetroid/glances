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
        if entry.mountpoint.starts_with(prefix) {
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
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
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
            let mut obj = BTreeMap::new();
            obj.insert("mntpoint".into(), Value::String(m.mountpoint));
            obj.insert("device".into(), Value::String(m.device));
            obj.insert("fstype".into(), Value::String(m.fstype));
            obj.insert("total".into(), Value::Uint(usage.total));
            obj.insert("used".into(), Value::Uint(usage.used));
            obj.insert("free".into(), Value::Uint(usage.free));
            obj.insert("percent".into(), Value::Float(usage.percent));
            out.push(Value::Object(obj));
        }
        self.base.stats = Value::Array(out);
        Ok(())
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