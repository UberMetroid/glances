use crate::core::timer::{Counter, Timer};
use std::thread::sleep;
use std::time::Duration;

#[test]
fn timer_zero_duration_always_finished() {
    let t = Timer::new(0.0);
    assert!(t.finished());
}

#[test]
fn timer_waits_then_resets() {
    let mut t = Timer::new(0.05);
    assert!(!t.finished());
    sleep(Duration::from_millis(80));
    assert!(t.finished());
    t.reset();
    assert!(!t.finished());
}

#[test]
fn timer_set_changes_duration() {
    let mut t = Timer::new(60.0);
    t.set(0.01);
    sleep(Duration::from_millis(30));
    assert!(t.finished());
}

#[test]
fn counter_increments() {
    let mut c = Counter::new();
    assert_eq!(c.value(), 0);
    c.inc(); c.inc(); c.inc();
    assert_eq!(c.value(), 3);
}
