//! Event log — mirrors `glances/events_list.py` (367 LOC).
//!
//! M1 implementation: bounded ring buffer with severity-tagged entries.
//! Used by the alert plugin in M11 and by `get_alert()` in the plugin base
//! (M1-followup).

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

pub struct EventLog {
    entries: VecDeque<Event>,
    max: usize,
}

impl EventLog {
    pub fn new(max: usize) -> Self {
        Self { entries: VecDeque::with_capacity(max), max }
    }

    pub fn push(&mut self, e: Event) {
        // A zero capacity means "keep nothing" — previously it stored
        // exactly one entry because pop ran before the push.
        if self.max == 0 {
            return;
        }
        if self.entries.len() >= self.max {
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

    /// Upstream `GlancesEventsList.clean(critical=False)` parity: drop
    /// finished WARNING entries, keeping CRITICAL ones unless
    /// `critical` is set. The port logs point-in-time threshold
    /// crossings (no start/end duration tracking), so every entry
    /// counts as finished — severity is the only retention signal.
    pub fn clean(&mut self, critical: bool) {
        let keep_critical = !critical;
        self.entries.retain(|e| match e.severity {
            Severity::Warning => false,
            Severity::Critical => keep_critical,
            _ => true,
        });
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
    #[test]
    fn ring_buffer_evicts_oldest() {
        let mut log = EventLog::new(2);
        for i in 0..3 {
            log.push(Event { severity: Severity::Ok, stat: format!("s{}", i), value: i as f64, timestamp: SystemTime::now() });
        }
        assert_eq!(log.len(), 2);
        assert_eq!(log.snapshot()[0].stat, "s1");
    }
    #[test]
    fn cap_zero_stores_nothing() {
        let mut log = EventLog::new(0);
        log.push(Event { severity: Severity::Ok, stat: "x".into(), value: 1.0, timestamp: SystemTime::now() });
        assert_eq!(log.len(), 0);
        assert!(log.is_empty());
    }
    #[test]
    fn clear_empties() {
        let mut log = EventLog::default();
        log.push(Event { severity: Severity::Warning, stat: "x".into(), value: 1.0, timestamp: SystemTime::now() });
        log.clear();
        assert!(log.is_empty());
    }
    fn entry(sev: Severity) -> Event {
        Event { severity: sev, stat: "x".into(), value: 1.0, timestamp: SystemTime::now() }
    }
    #[test]
    fn clean_warning_keeps_critical() {
        let mut log = EventLog::default();
        log.push(entry(Severity::Warning));
        log.push(entry(Severity::Critical));
        log.push(entry(Severity::Ok));
        log.clean(false);
        let kept: Vec<Severity> = log.snapshot().iter().map(|e| e.severity).collect();
        assert_eq!(kept, vec![Severity::Critical, Severity::Ok]);
    }
    #[test]
    fn clean_all_drops_critical() {
        let mut log = EventLog::default();
        log.push(entry(Severity::Warning));
        log.push(entry(Severity::Critical));
        log.clean(true);
        assert!(log.is_empty());
    }
}
