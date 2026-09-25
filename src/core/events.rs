//! Severity log: a bounded record of threshold crossings.
//!
//! The alert engine appends point-in-time entries here; the alert
//! plugin surfaces them and the clear endpoints prune them.

use std::collections::VecDeque;
use std::time::SystemTime;

use super::threshold::Severity;

#[derive(Debug, Clone)]
pub struct Event {
    pub severity: Severity,
    pub stat: String,
    pub value: f64,
    pub timestamp: SystemTime,
}

/// Oldest-first ring buffer. A zero cap keeps nothing; otherwise the
/// oldest entry is evicted once the log is full.
pub struct EventLog {
    entries: VecDeque<Event>,
    max: usize,
}

impl EventLog {
    pub fn new(max: usize) -> Self {
        Self { entries: VecDeque::with_capacity(max), max }
    }

    pub fn push(&mut self, e: Event) {
        if self.max == 0 {
            return;
        }
        while self.entries.len() >= self.max {
            self.entries.pop_front();
        }
        self.entries.push_back(e);
    }

    pub fn snapshot(&self) -> Vec<Event> {
        self.entries.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Prune by severity. Warning-level clears drop the two lower alert
    /// bands (Warning, Careful) while keeping Critical history and Ok
    /// baselines; a full clear drops everything. Entries are point-in-
    /// time crossings with no lifetime tracking, so every entry is
    /// eligible — severity is the only signal.
    pub fn clean(&mut self, critical: bool) {
        if critical {
            self.entries.clear();
            return;
        }
        let minor = |e: &Event| matches!(e.severity, Severity::Warning | Severity::Careful);
        self.entries.retain(|e| !minor(e));
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

impl Default for EventLog {
    fn default() -> Self { Self::new(100) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(sev: Severity, stat: &str) -> Event {
        Event { severity: sev, stat: stat.into(), value: 1.0, timestamp: SystemTime::now() }
    }
    #[test]
    fn oldest_falls_off_at_capacity() {
        let mut log = EventLog::new(2);
        log.push(at(Severity::Ok, "a"));
        log.push(at(Severity::Ok, "b"));
        log.push(at(Severity::Ok, "c"));
        let stats: Vec<String> = log.snapshot().iter().map(|e| e.stat.clone()).collect();
        assert_eq!(stats, vec!["b".to_string(), "c".to_string()]);
    }
    #[test]
    fn zero_capacity_keeps_nothing() {
        let mut log = EventLog::new(0);
        log.push(at(Severity::Critical, "a"));
        assert_eq!(log.len(), 0);
        assert!(log.is_empty());
    }
    #[test]
    fn warning_clean_drops_minor_bands_only() {
        let mut log = EventLog::default();
        for sev in [Severity::Warning, Severity::Careful, Severity::Critical, Severity::Ok] {
            log.push(at(sev, "a"));
        }
        log.clean(false);
        let kept: Vec<Severity> = log.snapshot().iter().map(|e| e.severity).collect();
        assert_eq!(kept, vec![Severity::Critical, Severity::Ok]);
    }
    #[test]
    fn full_clean_empties_the_log() {
        let mut log = EventLog::default();
        for sev in [Severity::Warning, Severity::Careful, Severity::Critical, Severity::Ok] {
            log.push(at(sev, "a"));
        }
        log.clean(true);
        assert!(log.is_empty());
        log.push(at(Severity::Ok, "a"));
        log.clear();
        assert!(log.is_empty());
    }
}
