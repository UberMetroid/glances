//! Timer + Counter — mirrors `glances/timer.py`.
//!
//! Used by `_check_decorator` equivalent (`model.py:1283`) to gate how
//! often a plugin actually re-reads its data source.

use std::time::Instant;

/// Countdown timer. `finished()` returns true once `duration` seconds have
/// elapsed since the last `reset()`.
pub struct Timer {
    duration: f32,
    last_reset: Instant,
}

impl Timer {
    pub fn new(duration: f32) -> Self {
        Self { duration, last_reset: Instant::now() }
    }

    pub fn reset(&mut self) {
        self.last_reset = Instant::now();
    }

    pub fn set(&mut self, duration: f32) {
        self.duration = duration;
        self.reset();
    }

    pub fn finished(&self) -> bool {
        if self.duration <= 0.0 { return true; }
        self.last_reset.elapsed().as_secs_f32() >= self.duration
    }

    pub fn elapsed_secs(&self) -> f32 {
        self.last_reset.elapsed().as_secs_f32()
    }
}

/// Monotonic counter (matches `Counter` in `glances/timer.py:48`).
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
    fn timer_zero_duration_is_always_finished() {
        let t = Timer::new(0.0);
        assert!(t.finished());
    }
    #[test]
    fn timer_waits() {
        let mut t = Timer::new(0.05);
        assert!(!t.finished());
        sleep(Duration::from_millis(80));
        assert!(t.finished());
        t.reset();
        assert!(!t.finished());
    }
    #[test]
    fn counter_increments() {
        let mut c = Counter::new();
        assert_eq!(c.value(), 0);
        c.inc(); c.inc(); c.inc();
        assert_eq!(c.value(), 3);
    }
}
