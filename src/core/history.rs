//! Per-key history: bounded timestamped sample buffers.
//!
//! Each plugin owns one of these and appends a sample per series on
//! every tick. Oldest samples fall off once a series passes the cap.

use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

/// One reading: when it was taken and what it was.
#[derive(Debug, Clone)]
pub struct Sample {
    pub timestamp: SystemTime,
    pub value: f64,
}

/// Key → recent samples, each series capped (default 28800 — a full day
/// of 3-second ticks with headroom).
pub struct GlancesHistory {
    entries: HashMap<String, Vec<Sample>>,
    max_size: usize,
}

impl GlancesHistory {
    pub fn new() -> Self {
        Self::with_capacity(28800)
    }

    pub fn with_capacity(max_size: usize) -> Self {
        Self { entries: HashMap::new(), max_size }
    }

    /// Append a sample, dropping the oldest while over the cap.
    pub fn add(&mut self, key: &str, value: f64) {
        let series = self.entries.entry(key.to_string()).or_default();
        series.push(Sample { timestamp: SystemTime::now(), value });
        if series.len() > self.max_size {
            series.drain(..series.len() - self.max_size);
        }
    }

    /// The newest `nb` samples (`nb == 0` returns the whole series;
    /// unknown keys return nothing).
    pub fn get(&self, key: &str, nb: usize) -> Vec<Sample> {
        let Some(series) = self.entries.get(key) else { return Vec::new() };
        if nb == 0 || nb >= series.len() {
            series.clone()
        } else {
            series[series.len() - nb..].to_vec()
        }
    }

    /// Forget every series.
    pub fn reset(&mut self) {
        self.entries.clear();
    }

    /// Resize the per-series cap (floored at 1 so the store stays usable).
    pub fn set_max_size(&mut self, max_size: usize) {
        self.max_size = max_size.max(1);
    }

    /// Export every series as (epoch seconds, value) pairs.
    pub fn snapshot(&self) -> BTreeMap<String, Vec<(f64, f64)>> {
        self.entries
            .iter()
            .map(|(k, series)| {
                let pts = series.iter().map(|s| (epoch(s.timestamp), s.value)).collect();
                (k.clone(), pts)
            })
            .collect()
    }

    /// Slope between the newest two samples. Needs two samples and a
    /// positive time gap; anything else yields 0.0.
    pub fn rate(&self, key: &str) -> f64 {
        let [prev, last] = match self.entries.get(key).map(Vec::as_slice) {
            Some([.., p, l]) => [p, l],
            _ => return 0.0,
        };
        let secs = last.timestamp.duration_since(prev.timestamp).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        if secs > 0.0 { (last.value - prev.value) / secs } else { 0.0 }
    }
}

impl Default for GlancesHistory {
    fn default() -> Self { Self::new() }
}

fn epoch(t: SystemTime) -> f64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn newest_samples_win_on_overflow() {
        let mut h = GlancesHistory::with_capacity(3);
        for v in [1.0, 2.0, 3.0, 4.0] {
            h.add("k", v);
        }
        let vals: Vec<f64> = h.get("k", 0).iter().map(|s| s.value).collect();
        assert_eq!(vals, vec![2.0, 3.0, 4.0]);
    }
    #[test]
    fn tail_slice_and_unknown_key() {
        let mut h = GlancesHistory::new();
        for v in [1.0, 2.0, 3.0] {
            h.add("k", v);
        }
        assert_eq!(h.get("k", 2).len(), 2);
        assert!(h.get("nope", 0).is_empty());
        h.reset();
        assert!(h.get("k", 0).is_empty());
    }
    #[test]
    fn rate_needs_two_samples() {
        let mut h = GlancesHistory::new();
        assert_eq!(h.rate("k"), 0.0);
        h.add("k", 5.0);
        assert_eq!(h.rate("k"), 0.0);
    }
    #[test]
    fn cap_floors_at_one() {
        let mut h = GlancesHistory::new();
        h.set_max_size(0);
        h.add("k", 1.0);
        h.add("k", 2.0);
        assert_eq!(h.get("k", 0).len(), 1);
    }
}
