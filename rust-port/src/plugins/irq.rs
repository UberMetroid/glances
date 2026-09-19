//! Per-IRQ-line stats — one entry per interrupt line from /proc/interrupts.
//!
//! Mirrors `glances/plugins/irq/__init__.py`. Linux-only. Each entry holds
//! the per-CPU counts of the IRQ, its number, and its short description.
//! If /proc/interrupts can't be read, returns an empty array (the plugin
//! stays enabled but reports nothing useful — matches Python behavior).

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::{GlancesError, Result};
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "irq";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(IrqPlugin::new()));
}

pub struct IrqPlugin { base: GlancesPluginModel }

impl IrqPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

/// Read and parse /proc/interrupts. Returns Err on IO failure.
pub fn read_interrupts() -> Result<Vec<BTreeMap<String, Value>>> {
    let text = fs::read_to_string("/proc/interrupts").map_err(GlancesError::Io)?;
    parse(&text)
}

/// Parse a /proc/interrupts snapshot. Public for testing with fixtures.
/// Format:
///           CPU0       CPU1       ...
///    0:    1234           0   IR-IO-APIC   timer
///    1:       0         567   IR-IO-APIC   i8042
pub fn parse(text: &str) -> Result<Vec<BTreeMap<String, Value>>> {
    let mut out: Vec<BTreeMap<String, Value>> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() { continue; }
        let trimmed = line.trim_start();
        // Header lines start with "CPU" — skip them.
        if trimmed.starts_with("CPU") { continue; }
        // Each data line: "<irq>: <num> <num> ... <type> <name...>"
        let colon = match line.find(':') {
            Some(c) => c,
            None => continue,
        };
        let irq_str = line[..colon].trim();
        // Skip non-numeric IRQ lines (could be "ERR:" or "MIS:" counters).
        if irq_str.parse::<u64>().is_err() { continue; }
        let after = line[colon + 1..].trim();
        // Split first chunk of whitespace-delimited tokens: counts then "type name".
        let mut parts = after.split_whitespace();
        let mut counts: Vec<u64> = Vec::new();
        // The line may have a non-numeric token at the end ("type name") — the
        // boundary is when parse::<u64> fails. For robustness, just collect
        // every leading token that parses.
        loop {
            match parts.next() {
                Some(s) => {
                    if let Ok(n) = s.parse::<u64>() {
                        counts.push(n);
                    } else {
                        // This token + everything remaining is the "type name" tail.
                        let mut tail = String::from(s);
                        for extra in parts {
                            tail.push(' ');
                            tail.push_str(extra);
                        }
                        let mut m: BTreeMap<String, Value> = BTreeMap::new();
                        m.insert("irq_number".into(), Value::String(irq_str.to_string()));
                        m.insert("count".into(), Value::Uint(counts.iter().sum()));
                        for (i, c) in counts.iter().enumerate() {
                            m.insert(format!("cpu{}", i), Value::Uint(*c));
                        }
                        m.insert("type".into(), Value::String(tail));
                        out.push(m);
                        break;
                    }
                }
                None => {
                    // No type/name tail — just counts.
                    let mut m: BTreeMap<String, Value> = BTreeMap::new();
                    m.insert("irq_number".into(), Value::String(irq_str.to_string()));
                    m.insert("count".into(), Value::Uint(counts.iter().sum()));
                    for (i, c) in counts.iter().enumerate() {
                        m.insert(format!("cpu{}", i), Value::Uint(*c));
                    }
                    out.push(m);
                    break;
                }
            }
        }
    }
    Ok(out)
}

impl Plugin for IrqPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn get_key(&self) -> Option<&'static str> { Some("irq_number") }

    fn update(&mut self) -> Result<()> {
        if cfg!(target_os = "linux") {
            match read_interrupts() {
                Ok(rows) => {
                    let mut arr: Vec<Value> = Vec::with_capacity(rows.len());
                    for r in rows {
                        arr.push(Value::Object(r));
                    }
                    self.base.stats = Value::Array(arr);
                }
                Err(_) => {
                    // Stay graceful: return an empty array.
                    self.base.stats = Value::Array(Vec::new());
                }
            }
        } else {
            self.base.stats = Value::Array(Vec::new());
        }
        Ok(())
    }
}