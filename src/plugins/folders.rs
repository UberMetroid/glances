//! Folders plugin — recursive size of user-configured directories.
//!
//! Glances' Python plugin walks a list of paths from the config file and
//! sums the bytes of every regular file beneath each one. We mirror that
//! behaviour using `std::fs::read_dir` recursively.
//!
//! Output is a `Value::Object` keyed by absolute folder path, with the
//! value being the total bytes consumed by regular files underneath.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::core::error::{GlancesError, Result};
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "folders";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(FoldersPlugin::new()));
}

/// Maximum recursion depth. Keeps a runaway symlink loop from blowing the
/// stack and prevents pathological filesystem layouts from holding the
/// refresh loop hostage.
pub const MAX_DEPTH: usize = 64;

/// Folders configured to be monitored. Hardcoded empty list for now —
/// M6-followup will wire this into `Args` / config.
pub fn configured_folders() -> Vec<String> {
    Vec::new()
}

/// Recursively sum the bytes of every regular file under `root`.
/// Symlinks are not followed (avoids loops). Subdirectory permission
/// errors abort that subtree but don't fail the whole walk. Returns
/// `Err` only if `root` itself can't be stat'd.
pub fn dir_size(root: &Path) -> Result<u64> {
    let meta = fs::symlink_metadata(root).map_err(GlancesError::Io)?;
    if meta.is_file() {
        return Ok(meta.len());
    }
    if !meta.is_dir() {
        // socket, fifo, block-dev, etc. — report 0 bytes.
        return Ok(0);
    }
    walk(root, 0)
}

fn walk(dir: &Path, depth: usize) -> Result<u64> {
    if depth > MAX_DEPTH {
        return Ok(0);
    }
    let mut total = 0u64;
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(0), // permission denied / vanished → skip subtree
    };
    for ent in entries {
        let ent = match ent {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = ent.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            // Don't follow symlinks; the size of the link itself is ~0 and
            // following could create a cycle.
            continue;
        }
        if ft.is_file() {
            total = total.saturating_add(meta.len());
        } else if ft.is_dir() {
            total = total.saturating_add(walk(&path, depth + 1)?);
        }
    }
    Ok(total)
}

pub struct FoldersPlugin { base: GlancesPluginModel }

impl Default for FoldersPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl FoldersPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Object(BTreeMap::new())) }
    }
}

impl Plugin for FoldersPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        let folders = configured_folders();
        let mut out = BTreeMap::new();
        for f in &folders {
            let p = Path::new(f);
            // Missing folder → 0 bytes, not an error (matches Python Glances).
            let bytes = dir_size(p).unwrap_or(0);
            out.insert(f.clone(), Value::Uint(bytes));
        }
        self.base.stats = Value::Object(out);
        Ok(())
    }
}