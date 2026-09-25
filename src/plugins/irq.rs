//! Per-IRQ-line stats — interrupt rates from /proc/interrupts.
//!
//! Mirrors `glances/plugins/irq/__init__.py`. Linux-only and disabled
//! by default upstream (`[irq] disable=True` in glances.conf) — the
//! registry only registers it when explicitly enabled.
//!
//! Schema per row (Python parity):
//!   - `irq_line`: human name — `<num>_<last word>` for numeric lines
//!     (`1_i8042`), the bare label for named lines (`LOC`, `NMI`, …).
//!   - `irq_rate`: interrupts per second, delta over wall-clock time.
//!   - `count`: cumulative count since boot (utility field).
//!
//! Rows are sorted by rate and capped at the top 5 (Python behavior).

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "irq";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(IrqPlugin::new()));
}

/// Human IRQ name per Python `__humanname`: numeric lines get
/// `<num>_<last word>` (`1_i8042`); named lines keep their label
/// (`LOC`, `NMI`). Returns None for headers/blank lines.
fn irq_line_name(line: &str) -> Option<String> {
    let colon = line.find(':')?;
    let label = line[..colon].trim();
    if label.is_empty() { return None; }
    if label.starts_with("CPU") { return None; }
    if label.chars().all(|c| c.is_ascii_digit()) {
        let last = line.split_whitespace().last().unwrap_or("");
        // A trailing purely-numeric last word means the line has no
        // name column — keep just the number.
        if last.chars().all(|c| c.is_ascii_digit()) || last.is_empty() {
            Some(label.to_string())
        } else {
            Some(format!("{}_{}", label, last))
        }
    } else {
        Some(label.to_string())
    }
}

/// Sum of the leading numeric columns (per-CPU counts) on a line.
fn irq_sum(line: &str) -> u64 {
    let colon = match line.find(':') { Some(c) => c, None => return 0 };
    line[colon + 1..]
        .split_whitespace()
        .take_while(|s| s.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|s| s.parse::<u64>().ok())
        .sum()
}

/// Parse /proc/interrupts into `(irq_line, cumulative_count)` pairs.
/// Public for fixture tests. Named lines (LOC/NMI/RES/ERR/MIS/…) are
/// kept — Python includes them and they carry the heaviest rates.
pub fn parse(text: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("CPU") { continue; }
        if let Some(name) = irq_line_name(line) {
            out.push((name, irq_sum(line)));
        }
    }
    out
}

pub struct IrqPlugin {
    base: GlancesPluginModel,
    lasts: HashMap<String, u64>,
    prev_time: Option<std::time::Instant>,
}

impl Default for IrqPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl IrqPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
            lasts: HashMap::new(),
            prev_time: None,
        }
    }
}

impl Plugin for IrqPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn get_key(&self) -> Option<&'static str> { Some("irq_line") }

    fn update(&mut self) -> Result<()> {
        let now = std::time::Instant::now();
        let dt = self.prev_time
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);
        self.prev_time = Some(now);

        let rows = match fs::read_to_string("/proc/interrupts") {
            Ok(text) => parse(&text),
            Err(_) => Vec::new(), // OpenVZ & friends lack the file.
        };

        let mut out: Vec<Value> = Vec::with_capacity(rows.len());
        for (name, cur) in rows {
            let rate = match (self.lasts.get(&name), dt > 0.0) {
                (Some(&prev), true) => cur.saturating_sub(prev) as f64 / dt,
                _ => 0.0,
            };
            self.lasts.insert(name.clone(), cur);
            let mut m: BTreeMap<String, Value> = BTreeMap::new();
            m.insert("irq_line".into(), Value::String(name));
            m.insert("irq_rate".into(), Value::Float(rate));
            m.insert("count".into(), Value::Uint(cur));
            out.push(Value::Object(m));
        }
        // Top 5 by rate (Python behavior); stable order for equal rates.
        out.sort_by(|a, b| {
            let ra = a.as_object().and_then(|o| o.get("irq_rate")).and_then(Value::as_f64).unwrap_or(0.0);
            let rb = b.as_object().and_then(|o| o.get("irq_rate")).and_then(Value::as_f64).unwrap_or(0.0);
            rb.partial_cmp(&ra).unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(5);
        self.base.stats = Value::Array(out);
        Ok(())
    }
}
