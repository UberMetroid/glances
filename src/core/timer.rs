//! Countdown timer plus a monotonic counter.
//!
//! Plugins use the timer to gate how often they re-read their data
//! source: poll the source when `finished`, otherwise serve the cache.

use std::time::Instant;

/// Elapses once `duration` seconds pass after creation or `reset`.
/// A non-positive duration is finished from the start.
pub struct Timer {
    duration: f32,
    started: Instant,
}

impl Timer {
    pub fn new(duration: f32) -> Self {
        Self { duration, started: Instant::now() }
    }

    pub fn reset(&mut self) {
        self.started = Instant::now();
    }

    pub fn set(&mut self, duration: f32) {
        self.duration = duration;
        self.reset();
    }

    pub fn finished(&self) -> bool {
        self.duration <= 0.0 || self.elapsed_secs() >= self.duration
    }

    pub fn elapsed_secs(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }
}

/// Counts up from zero, one step per `inc`.
pub struct Counter {
    value: u64,
}

impl Counter {
    pub fn new() -> Self { Self { value: 0 } }
    pub fn inc(&mut self) { self.value += 1; }
    pub fn value(&self) -> u64 { self.value }
}

impl Default for Counter {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;
    #[test]
    fn zero_and_negative_durations_are_finished() {
        assert!(Timer::new(0.0).finished());
        assert!(Timer::new(-5.0).finished());
    }
    #[test]
    fn positive_duration_elapses_then_resets() {
        let mut t = Timer::new(0.04);
        assert!(!t.finished());
        sleep(Duration::from_millis(70));
        assert!(t.finished());
        t.reset();
        assert!(!t.finished());
    }
    #[test]
    fn set_restarts_with_new_duration() {
        let mut t = Timer::new(100.0);
        t.set(0.0);
        assert!(t.finished());
    }
    #[test]
    fn counter_starts_at_zero() {
        let mut c = Counter::default();
        c.inc();
        c.inc();
        assert_eq!(c.value(), 2);
    }
}
