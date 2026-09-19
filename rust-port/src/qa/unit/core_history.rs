use crate::core::history::GlancesHistory;

#[test]
fn empty_returns_empty() {
    let h = GlancesHistory::new();
    assert!(h.get("missing", 10).is_empty());
}

#[test]
fn add_and_get() {
    let mut h = GlancesHistory::new();
    h.add("cpu.user", 10.0);
    h.add("cpu.user", 20.0);
    let s = h.get("cpu.user", 0);
    assert_eq!(s.len(), 2);
    assert_eq!(s[0].value, 10.0);
    assert_eq!(s[1].value, 20.0);
}

#[test]
fn get_n_truncates() {
    let mut h = GlancesHistory::new();
    for i in 0..5 { h.add("x", i as f64); }
    assert_eq!(h.get("x", 2).len(), 2);
    assert_eq!(h.get("x", 99).len(), 5);
}

#[test]
fn ring_buffer_evicts() {
    let mut h = GlancesHistory::with_capacity(3);
    for i in 0..5 { h.add("x", i as f64); }
    let s = h.get("x", 0);
    assert_eq!(s.len(), 3);
    assert_eq!(s[0].value, 2.0);
}

#[test]
fn reset_clears_all() {
    let mut h = GlancesHistory::new();
    h.add("a", 1.0);
    h.add("b", 2.0);
    h.reset();
    assert!(h.get("a", 0).is_empty());
    assert!(h.get("b", 0).is_empty());
}

#[test]
fn rate_with_fewer_than_two_is_zero() {
    let h = GlancesHistory::new();
    assert_eq!(h.rate("missing"), 0.0);
}
