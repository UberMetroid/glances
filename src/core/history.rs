//! GlancesHistory — per-stat ring buffer.
//!
//! Mirrors `glances/history.py:14` and `glances/attribute.py:22`. The
//! `GlancesAttribute` is collapsed into a single struct here to stay under
//! 256 lines.

use std::collections::HashMap;
use std::time::SystemTime;

/// Single timestamped sample.
#[derive(Debug, Clone)]
pub struct Sample {
    pub timestamp: SystemTime,
    pub value: f64,
}

/// Bounded per-key history. Default cap = 28800 (matches `model.py:735`).
pub struct GlancesHistory {
    entries: HashMap<String, Vec<Sample>>,
    max_size: usize,
}

impl GlancesHistory {
    pub fn new() -> Self {
        Self::with_capacity(28800)
    }

    pub fn with_capacity(max_size: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_size,
        }
    }

    /// Append a sample for `key`. If the buffer exceeds `max_size`, the
    /// oldest entry is dropped.
    pub fn add(&mut self, key: &str, value: f64) {
        let entry = self.entries.entry(key.to_string()).or_insert_with(Vec::new);
        entry.push(Sample { timestamp: SystemTime::now(), value });
        if entry.len() > self.max_size {
            let excess = entry.len() - self.max_size;
            entry.drain(0..excess);
        }
    }

    /// Return the last `nb` samples for `key` (0 = all).
    pub fn get(&self, key: &str, nb: usize) -> Vec<Sample> {
        match self.entries.get(key) {
            None => Vec::new(),
            Some(v) => {
                if nb == 0 || nb >= v.len() {
                    v.clone()
                } else {
                    v[v.len() - nb..].to_vec()
                }
            }
        }
    }

    /// Reset all history (matches `GlancesHistory.reset()`).
    pub fn reset(&mut self) {
        self.entries.clear();
    }

    /// Export all recorded series as key → (epoch-seconds, value) pairs
    /// for the `/history` endpoint. Empty until ticks are recorded.
    pub fn snapshot(&self) -> std::collections::BTreeMap<String, Vec<(f64, f64)>> {
        let mut out = std::collections::BTreeMap::new();
        for (k, samples) in &self.entries {
            out.insert(
                k.clone(),
                samples
                    .iter()
                    .map(|s| {
                        let ts = s
                            .timestamp
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs_f64())
                            .unwrap_or(0.0);
                        (ts, s.value)
                    })
                    .collect(),
            );
        }
        out
    }

    /// Compute the rate (per second) between the last two samples.
    /// Returns 0.0 if fewer than two samples exist.
    pub fn rate(&self, key: &str) -> f64 {
        if let Some(v) = self.entries.get(key) {
            if v.len() >= 2 {
                let last = &v[v.len() - 1];
                let prev = &v[v.len() - 2];
                if let Ok(dt) = last.timestamp.duration_since(prev.timestamp) {
                    let secs = dt.as_secs_f64();
                    if secs > 0.0 {
                        return (last.value - prev.value) / secs;
                    }
                }
            }
        }
        0.0
    }
}

impl Default for GlancesHistory {
    fn default() -> Self { Self::new() }
}
