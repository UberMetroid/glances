//! IRQ plugin — top interrupt lines by per-second rate.
//!
//! Cumulative counts come from /proc/interrupts over wall time
//! (first tick zeros). Rows sort rate-descending (stable) and cap at
//! 5. Named lines (LOC, NMI, …) stay — they carry the heaviest rates.

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

/// Human line name: `<num>_<last word>` for numeric lines (`1_i8042`,
/// bare number when no name word follows), the bare label for named
/// lines. Headers and blanks yield None.
fn irq_line_name(line: &str) -> Option<String> {
    let label = line[..line.find(':')?].trim();
    if label.is_empty() || label.starts_with("CPU") {
        return None;
    }
    if !label.chars().all(|c| c.is_ascii_digit()) {
        return Some(label.to_string());
    }
    match line.split_whitespace().last() {
        Some(last) if !last.is_empty() && !last.chars().all(|c| c.is_ascii_digit()) => {
            Some(format!("{label}_{last}"))
        }
        _ => Some(label.to_string()),
    }
}

/// Leading numeric columns (per-CPU counts) summed; the sum stops at
/// the first non-numeric token.
fn irq_sum(line: &str) -> u64 {
    let Some(colon) = line.find(':') else { return 0 };
    line[colon + 1..]
        .split_whitespace()
        .take_while(|s| s.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|s| s.parse::<u64>().ok())
        .sum()
}

/// /proc/interrupts text → (line name, cumulative count) pairs.
pub fn parse(text: &str) -> Vec<(String, u64)> {
    text.lines()
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.starts_with("CPU"))
        .filter_map(|t| irq_line_name(t).map(|n| (n, irq_sum(t))))
        .collect()
}

pub struct IrqPlugin {
    base: GlancesPluginModel,
    lasts: HashMap<String, u64>,
    prev_time: Option<std::time::Instant>,
}

impl Default for IrqPlugin {
    fn default() -> Self { Self::new() }
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
        let dt = self.prev_time.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        self.prev_time = Some(now);
        // Some kernels (OpenVZ) lack the file — empty stats, no error.
        let text = fs::read_to_string("/proc/interrupts").unwrap_or_default();
        let mut out: Vec<Value> = Vec::new();
        for (name, cur) in parse(&text) {
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
        out.sort_by(|a, b| {
            let rate = |v: &Value| v.as_object().and_then(|o| o.get("irq_rate")).and_then(Value::as_f64).unwrap_or(0.0);
            rate(b).partial_cmp(&rate(a)).unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(5);
        self.base.stats = Value::Array(out);
        Ok(())
    }
}
