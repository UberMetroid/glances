//! Host-root filesystem view for containers (node-exporter style).
//!
//! Bare metal reads `/proc/mounts` and stats each mountpoint. In a
//! container that yields the container's mounts (overlay root plus
//! one bind per CDI-injected NVIDIA file), not the host's. With
//! `GLANCES_ROOTFS=/host` (host `/` bind-mounted at `/host`) we
//! instead read `{root}/proc/1/mounts` (host init's mount table),
//! `statvfs` `{root}{mnt}`, and display `{mnt}`. Unset (or `/`)
//! keeps stock behavior.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::platform as plat;

use super::fs::{parse_mounts, should_skip};

/// Resolved mount source: which table to read, which prefix to stat.
#[derive(Debug, Clone, PartialEq)]
pub struct RootFs {
    pub mounts_file: PathBuf,
    pub prefix: PathBuf,
}

/// Resolve from an env value without touching the environment
/// (testable; [`resolve`] wraps this with the real env read).
pub fn resolve_from(var: Option<&str>) -> RootFs {
    match var.map(PathBuf::from) {
        Some(p) if !p.as_os_str().is_empty() && p != Path::new("/") => RootFs {
            mounts_file: p.join("proc/1/mounts"),
            prefix: p,
        },
        _ => RootFs {
            mounts_file: PathBuf::from("/proc/mounts"),
            prefix: PathBuf::from("/"),
        },
    }
}

/// Resolve from `GLANCES_ROOTFS` (unset = stock bare-metal paths).
pub fn resolve() -> RootFs {
    resolve_from(std::env::var("GLANCES_ROOTFS").ok().as_deref())
}

/// Read mounts and stat each one. Display paths are always the
/// unprefixed host-view paths (`/boot`), even when statvfs runs
/// against `{prefix}/boot`. Mountpoints that aren't directories
/// (CDI file binds) are skipped — a file is not a filesystem.
pub fn read_mounts_under(root: &RootFs) -> Result<Vec<Value>> {
    let text = fs::read_to_string(&root.mounts_file).map_err(GlancesError::Io)?;
    let mounts = parse_mounts(&text);
    let mut out = Vec::new();
    for m in mounts {
        if should_skip(&m) { continue; }
        let full = root.prefix.join(m.mountpoint.trim_start_matches('/'));
        if !full.is_dir() { continue; }
        let usage = match plat::linux::statvfs::statvfs_path(&full.to_string_lossy()) {
            Ok(u) => u,
            Err(_) => continue, // vanished mount — skip silently
        };
        let mut obj = BTreeMap::new();
        obj.insert("key".into(), Value::String("mnt_point".into()));
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
    Ok(out)
}
