//! GPU display helpers — stable per-vendor ids plus row rendering.
//!
//! Probing yields PCI bus ids (`0000:01:00.0`); dashboards match
//! `gpu_id` against per-vendor numbers (`nvidia0`, `intel0`), so
//! `assign_gpu_ids` hands those out in enumeration order. Rows also
//! carry the upstream aliases `proc` (load %), `mem` (VRAM %), and
//! `temperature` (°C) next to the native detail fields.

use std::collections::BTreeMap;

use super::gpu::GpuInfo;
use crate::core::value::Value;

fn opt(v: Option<f64>) -> Value {
    match v {
        Some(x) => Value::Float(x),
        None => Value::Null,
    }
}

/// VRAM used percent from MiB counters. `None` unless both ends are
/// known and the total is positive — shared-memory iGPUs report Null.
pub fn mem_pct(used_mb: Option<f64>, total_mb: Option<f64>) -> Option<f64> {
    match (used_mb, total_mb) {
        (Some(u), Some(t)) if t > 0.0 => Some(u / t * 100.0),
        _ => None,
    }
}

/// Dashboard order: internal GPUs first, then by name.
pub fn sort_gpus(gpus: &mut [GpuInfo]) {
    gpus.sort_by(|a, b| {
        let ka = u8::from(a.kind != "internal");
        let kb = u8::from(b.kind != "internal");
        (ka, &a.name).cmp(&(kb, &b.name))
    });
}

/// Number cards per vendor in `(vendor, pci)` order (`nvidia0`,
/// `nvidia1`, `intel0`, ...), matching NVML index order
/// (PCI-ascending). Card order is NOT used: `cardN` follows probe
/// order, which need not be PCI-ascending. The prefix is the
/// lowercased vendor; anything outside `[a-z0-9]` falls back to
/// `gpu` so ids stay widget-matchable. Runs in `update()` before
/// sorting so ids are stable within a boot.
pub fn assign_gpu_ids(infos: &mut [GpuInfo]) {
    let mut order: Vec<usize> = (0..infos.len()).collect();
    order.sort_by(|&a, &b| {
        (&infos[a].vendor, &infos[a].pci).cmp(&(&infos[b].vendor, &infos[b].pci))
    });
    let mut counters: BTreeMap<String, usize> = BTreeMap::new();
    for i in order {
        let g = &mut infos[i];
        let v = g.vendor.to_ascii_lowercase();
        let prefix = if !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric()) {
            v
        } else {
            "gpu".to_string()
        };
        let n = counters.entry(prefix.clone()).or_insert(0);
        g.gpu_id = format!("{prefix}{n}");
        *n += 1;
    }
}

/// One card as a JSON object: identity keys (`key`, `gpu_id`, `pci`,
/// `vendor`, `name`, `kind`), upstream aliases (`proc`, `mem`,
/// `temperature`), native readings, `clients` (pid, name, service,
/// NVIDIA-only mem_mb, per-client transcoding), and the card-level
/// `transcoding` / `transcoding_by` pair.
pub fn gpu_to_value(g: &GpuInfo) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("key".into(), Value::String("gpu_id".into()));
    obj.insert("gpu_id".into(), Value::String(g.gpu_id.clone()));
    obj.insert("pci".into(), Value::String(g.pci.clone()));
    obj.insert("vendor".into(), Value::String(g.vendor.clone()));
    obj.insert("name".into(), Value::String(g.name.clone()));
    obj.insert("kind".into(), Value::String(g.kind.clone()));
    obj.insert("proc".into(), opt(g.util_pct));
    obj.insert("mem".into(), opt(mem_pct(g.mem_used_mb, g.mem_total_mb)));
    obj.insert("temperature".into(), opt(g.temp_c));
    obj.insert("util_pct".into(), opt(g.util_pct));
    obj.insert("freq_mhz".into(), opt(g.freq_mhz));
    obj.insert("mem_used_mb".into(), opt(g.mem_used_mb));
    obj.insert("mem_total_mb".into(), opt(g.mem_total_mb));
    obj.insert("temp_c".into(), opt(g.temp_c));
    let clients: Vec<Value> = g
        .clients
        .iter()
        .map(|c| {
            let mut o = BTreeMap::new();
            o.insert("pid".into(), Value::Uint(c.pid as u64));
            o.insert("name".into(), Value::String(c.name.clone()));
            o.insert(
                "service".into(),
                c.service.clone().map(Value::String).unwrap_or(Value::Null),
            );
            o.insert("mem_mb".into(), opt(c.mem_mb));
            o.insert("transcoding".into(), Value::Bool(c.transcoding));
            Value::Object(o)
        })
        .collect();
    obj.insert("clients".into(), Value::Array(clients));
    obj.insert("transcoding".into(), Value::Bool(g.transcoding));
    obj.insert(
        "transcoding_by".into(),
        g.transcoding_by.clone().map(Value::String).unwrap_or(Value::Null),
    );
    Value::Object(obj)
}
