//! Tests for the IRQ plugin.

use crate::core::plugin::Plugin;
use crate::core::value::Value;
use crate::plugins::irq;

const SAMPLE_INTERRUPTS: &str = "\
           CPU0       CPU1       CPU2       CPU3
   0:        100          0          0          0   IR-IO-APIC   2-edge      timer
   1:          0         50          0          0   IR-IO-APIC   1-edge      i8042
   6:          2          0          0          0   IR-IO-APIC   6-edge      AMDI0010
  10:        17         23          5         19   IR-PCI-MSI   0-edge      eth0
";

const SINGLE_CPU: &str = "\
          CPU0
   0:        100   IR-IO-APIC   timer
   1:         50   IR-IO-APIC   i8042
";

const TRUNCATED: &str = "\
           CPU0       CPU1
   0:        100
   badline missing colon
";

const EMPTY_HEADER: &str = "\
           CPU0       CPU1
";

#[test]
fn plugin_metadata() {
    let p = irq::IrqPlugin::new();
    assert_eq!(p.name(), irq::NAME);
    assert_eq!(p.name(), "irq");
    assert_eq!(p.get_key(), Some("irq_number"));
}

#[test]
fn parse_produces_per_irq_rows() {
    let rows = irq::parse(SAMPLE_INTERRUPTS).expect("parse should succeed");
    assert_eq!(rows.len(), 4);
    // IRQ 0 should have count = 100 across all CPUs.
    let r0 = rows.iter().find(|r| r.get("irq_number").and_then(|v| v.as_str()) == Some("0")).unwrap();
    assert_eq!(r0.get("count"), Some(&Value::Uint(100)));
    assert_eq!(r0.get("cpu0"), Some(&Value::Uint(100)));
    assert_eq!(r0.get("cpu3"), Some(&Value::Uint(0)));
    assert!(r0.get("type").and_then(|v| v.as_str()).unwrap().contains("timer"));
}

#[test]
fn parse_single_cpu() {
    let rows = irq::parse(SINGLE_CPU).expect("parse should succeed");
    assert_eq!(rows.len(), 2);
    let r0 = &rows[0];
    assert_eq!(r0.get("irq_number"), Some(&Value::String("0".into())));
    assert_eq!(r0.get("count"), Some(&Value::Uint(100)));
    assert!(r0.get("type").and_then(|v| v.as_str()).unwrap().contains("timer"));
}

#[test]
fn parse_handles_truncated_lines() {
    let rows = irq::parse(TRUNCATED).expect("parse should not fail on truncated input");
    // Should skip the header and the "badline" (no colon) and still parse IRQ 0.
    assert_eq!(rows.len(), 1);
    let r0 = &rows[0];
    assert_eq!(r0.get("irq_number"), Some(&Value::String("0".into())));
    assert_eq!(r0.get("count"), Some(&Value::Uint(100)));
    // No "type" because the line was truncated before the type column.
    assert!(r0.get("type").is_none() || r0.get("type") == Some(&Value::String(String::new())));
}

#[test]
fn parse_empty_header_only() {
    let rows = irq::parse(EMPTY_HEADER).expect("parse should not fail on header-only input");
    assert!(rows.is_empty(), "no data rows => empty array");
}

#[test]
fn parse_skips_non_numeric_irq_lines() {
    let text = "          CPU0\nERR: 1\nMIS: 2\n   1: 5 IR-IO-APIC   timer\n";
    let rows = irq::parse(text).expect("parse should succeed");
    // Only the numeric "1" should produce a row.
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("irq_number"), Some(&Value::String("1".into())));
}

#[test]
fn update_on_linux_does_not_panic() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = irq::IrqPlugin::new();
    p.update().expect("irq update should succeed on Linux");
    // The result is an array; we don't assert content because the host kernel
    // may not have any IRQs registered.
    let _ = p.stats().as_array().expect("stats must be an array");
}