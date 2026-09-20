//! MPP plugin — Rockchip Media Process Platform encoder/decoder load.
//!
//! Rockchip exposes the encoder and decoder utilization under
//! /sys/kernel/debug/dri/<N>/{enc_load,dec_load,load_monitor}. The
//! kernel debugfs must be mounted and we must have read permission
//! (usually root). On hosts without either, the plugin returns an
//! empty array — same JSON shape, no error.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "mpp";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(MppPlugin::new()));
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MppChannel {
    pub channel_id: String,
    pub encoder_load_pct: Option<f64>,
    pub decoder_load_pct: Option<f64>,
}

/// Read a single integer percentage from a debugfs file.
fn read_pct(path: &Path) -> Option<f64> {
    let s = fs::read_to_string(path).ok()?;
    let n: f64 = s.trim().parse().ok()?;
    if n < 0.0 { return None; }
    Some(n)
}

/// Read the load_monitor file. Format (Rockchip BSP):
///   "Encoder: 12 Decoder: 5"   — two space-separated values.
/// Some kernels instead publish separate enc_load / dec_load files.
pub fn read_load_monitor(path: &Path) -> Option<(f64, f64)> {
    let text = fs::read_to_string(path).ok()?;
    let mut enc = None;
    let mut dec = None;
    for tok in text.split_whitespace() {
        // tokens like "Encoder:" / "Decoder:" — skip the label.
        if tok.contains(':') { continue; }
        if let Ok(n) = tok.trim_end_matches(',').parse::<f64>() {
            if enc.is_none() { enc = Some(n); }
            else if dec.is_none() { dec = Some(n); break; }
        }
    }
    match (enc, dec) {
        (Some(e), Some(d)) => Some((e, d)),
        _ => None,
    }
}

/// Probe one `/sys/kernel/debug/dri/N` directory. Returns None if
/// neither load_monitor nor enc_load / dec_load files exist (not a
/// Rockchip MPP node, or unreadable).
pub fn probe_dri(dri_dir: &Path) -> Option<MppChannel> {
    let id = dri_dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dri0".to_string());

    let mut enc_load = read_pct(&dri_dir.join("enc_load"));
    let mut dec_load = read_pct(&dri_dir.join("dec_load"));

    // Prefer the combined file when present — it carries both values.
    if let Some((e, d)) = read_load_monitor(&dri_dir.join("load_monitor")) {
        if enc_load.is_none() { enc_load = Some(e); }
        if dec_load.is_none() { dec_load = Some(d); }
    }

    if enc_load.is_none() && dec_load.is_none() { return None; }
    Some(MppChannel {
        channel_id: id,
        encoder_load_pct: enc_load,
        decoder_load_pct: dec_load,
    })
}

/// Enumerate `/sys/kernel/debug/dri/*` directories. If debugfs is not
/// mounted (or unreadable) the function returns an empty Vec.
pub fn list_dri() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let entries = match fs::read_dir("/sys/kernel/debug/dri") {
        Ok(e) => e,
        Err(_) => return out,
    };
    for ent in entries.flatten() {
        let p = ent.path();
        if p.is_dir() { out.push(p); }
    }
    out.sort();
    out
}

pub fn mpp_to_value(c: &MppChannel) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("channel_id".into(), Value::String(c.channel_id.clone()));
    obj.insert("encoder_load_pct".into(), match c.encoder_load_pct {
        Some(v) => Value::Float(v),
        None => Value::Null,
    });
    obj.insert("decoder_load_pct".into(), match c.decoder_load_pct {
        Some(v) => Value::Float(v),
        None => Value::Null,
    });
    Value::Object(obj)
}

pub struct MppPlugin { base: GlancesPluginModel }

impl MppPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

impl Default for MppPlugin {
    fn default() -> Self { Self::new() }
}

impl Plugin for MppPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn history_items(&self) -> &[&'static str] { &["load"] }
    fn get_key(&self) -> Option<&'static str> { Some("channel_id") }

    fn update(&mut self) -> Result<()> {
        // list_dri returns an empty Vec if debugfs isn't mounted or
        // isn't readable; the plugin must not error in that case.
        let mut out: Vec<Value> = Vec::new();
        for d in list_dri() {
            if let Some(ch) = probe_dri(&d) {
                out.push(mpp_to_value(&ch));
            }
        }
        self.base.stats = Value::Array(out);
        Ok(())
    }
}
