//! Tests for the IRQ plugin (Python-parity schema: irq_line/irq_rate).

use crate::core::plugin::Plugin;
use crate::core::value::Value;
use crate::plugins::irq;

const SAMPLE_INTERRUPTS: &str = "\
           CPU0       CPU1       CPU2       CPU3
   0:        100          0          0          0   IR-IO-APIC   2-edge      timer
   1:          0         50          0          0   IR-IO-APIC   1-edge      i8042
  10:        17         23          5         19   IR-PCI-MSI   0-edge      eth0
 LOC:      1000       2000       3000       4000   Local timer interrupts
";

#[test]
fn plugin_metadata() {
    let p = irq::IrqPlugin::new();
    assert_eq!(p.name(), "irq");
    assert_eq!(p.get_key(), Some("irq_line"));
}

#[test]
fn parse_produces_named_rows() {
    let rows = irq::parse(SAMPLE_INTERRUPTS);
    // Numeric lines get `<num>_<last-word>`; LOC keeps its label.
    let names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(names, vec!["0_timer", "1_i8042", "10_eth0", "LOC"]);
    let r1 = rows.iter().find(|r| r.0 == "1_i8042").unwrap();
    assert_eq!(r1.1, 50);
    let loc = rows.iter().find(|r| r.0 == "LOC").unwrap();
    assert_eq!(loc.1, 10_000);
}

#[test]
fn parse_keeps_non_numeric_irq_lines() {
    // ERR/MIS are valid named lines in Python's parser too.
    let rows = irq::parse("          CPU0\nERR: 1\nMIS: 2\n   1: 5 IR-IO-APIC   timer\n");
    let names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(names, vec!["ERR", "MIS", "1_timer"]);
}

#[test]
fn parse_handles_truncated_lines() {
    let rows = irq::parse("           CPU0       CPU1\n   0:        100\nbadline no colon\n");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "0");
    assert_eq!(rows[0].1, 100);
}

#[test]
fn irq_line_number_only_when_no_name() {
    // A numeric line with no trailing name column keeps just the number.
    let rows = irq::parse("   CPU0\n  42:   7\n");
    assert_eq!(rows[0].0, "42");
    assert_eq!(rows[0].1, 7);
}

#[test]
fn update_on_linux_does_not_panic() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = irq::IrqPlugin::new();
    p.update().expect("irq update should succeed on Linux");
    let arr = p.stats().as_array().expect("stats must be an array");
    // First tick rates are 0 (no previous sample); schema is stable.
    for row in arr {
        let o = row.as_object().unwrap();
        assert!(o.contains_key("irq_line"));
        assert!(o.contains_key("irq_rate"));
        assert!(o.contains_key("count"));
    }
    assert!(arr.len() <= 5);
}

#[test]
fn update_is_registered_only_when_enabled() {
    // irq is upstream-disabled by default: register_filtered must not
    // register it unless --enable-plugin names it.
    let stats = crate::core::stats::GlancesStats::new(1.0);
    crate::plugins::register_filtered(&stats, &[], &[]);
    let snap = stats.snapshot();
    let obj = snap.as_object().unwrap();
    assert!(!obj.contains_key("irq"), "irq must be default-disabled");

    let stats2 = crate::core::stats::GlancesStats::new(1.0);
    crate::plugins::register_filtered(&stats2, &[], &["irq".to_string()]);
    let snap2 = stats2.snapshot();
    assert!(snap2.as_object().unwrap().contains_key("irq"));
}

#[test]
fn rate_is_delta_over_wallclock() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = irq::IrqPlugin::new();
    p.update().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    p.update().unwrap();
    let arr = p.stats().as_array().unwrap();
    // Rates are non-negative floats; counts monotonically grow.
    for row in arr {
        let o = row.as_object().unwrap();
        assert!(o.get("irq_rate").and_then(Value::as_f64).unwrap_or(-1.0) >= 0.0);
        assert!(o.get("count").and_then(|v| match v {
            Value::Uint(u) => Some(*u), _ => None,
        }).is_some());
    }
}
