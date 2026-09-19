//! Unit tests for core::threshold.

use crate::core::threshold::{evaluate, get_limit, Severity};
use std::collections::HashMap;

#[test]
fn ordering_chains() {
    assert!(Severity::Ok < Severity::Careful);
    assert!(Severity::Careful < Severity::Warning);
    assert!(Severity::Warning < Severity::Critical);
}

#[test]
fn evaluate_below_all() {
    assert_eq!(evaluate(10.0, Some(50.0), Some(70.0), Some(90.0)), Severity::Ok);
}

#[test]
fn evaluate_above_careful_only() {
    assert_eq!(evaluate(60.0, Some(50.0), Some(70.0), Some(90.0)), Severity::Careful);
}

#[test]
fn evaluate_above_warning_only() {
    assert_eq!(evaluate(80.0, Some(50.0), Some(70.0), Some(90.0)), Severity::Warning);
}

#[test]
fn evaluate_above_critical() {
    assert_eq!(evaluate(95.0, Some(50.0), Some(70.0), Some(90.0)), Severity::Critical);
}

#[test]
fn evaluate_no_thresholds() {
    assert_eq!(evaluate(99.0, None, None, None), Severity::Ok);
}

#[test]
fn get_limit_stat_specific_wins() {
    let mut limits = HashMap::new();
    limits.insert("cpu_user_careful".into(), 60.0);
    limits.insert("cpu_careful".into(), 50.0);
    assert_eq!(get_limit("cpu_user", "cpu", Severity::Careful, &limits), Some(60.0));
}

#[test]
fn get_limit_falls_back_to_plugin() {
    let mut limits = HashMap::new();
    limits.insert("cpu_careful".into(), 50.0);
    assert_eq!(get_limit("cpu_user", "cpu", Severity::Careful, &limits), Some(50.0));
}
