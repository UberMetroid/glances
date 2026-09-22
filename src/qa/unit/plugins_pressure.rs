//! Unit tests for the pressure plugin — PSI `some` parsing.

use crate::plugins::pressure::{parse_some_avg10, NAME};
use crate::plugins::pressure::PressurePlugin;
use crate::core::plugin::Plugin;

#[test]
fn parses_cpu_some_line() {
    let text = "some avg10=1.25 avg60=0.42 avg300=0.10 total=1234567\n";
    assert_eq!(parse_some_avg10(text), Some(1.25));
}

#[test]
fn skips_full_line_for_memory_and_io() {
    let text = "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n\
                full avg10=9.99 avg60=0.00 avg300=0.00 total=0\n";
    assert_eq!(parse_some_avg10(text), Some(0.0));
}

#[test]
fn returns_none_for_missing_or_broken_input() {
    assert_eq!(parse_some_avg10(""), None);
    assert_eq!(parse_some_avg10("full avg10=1.00 total=0\n"), None);
    assert_eq!(parse_some_avg10("some avg10=nope total=0\n"), None);
    assert_eq!(parse_some_avg10("some total=0\n"), None);
}

#[test]
fn plugin_registers_under_its_name_with_three_keys() {
    let p = PressurePlugin::new();
    assert_eq!(p.name(), NAME);
    let o = p.stats().as_object().expect("pressure stats object");
    assert!(o.contains_key("cpu") && o.contains_key("mem") && o.contains_key("io"));
}
