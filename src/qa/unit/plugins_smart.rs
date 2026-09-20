//! Tests for the smart plugin — smartctl output parsers (fixture-based,
//! no live binary required).

use crate::plugins::smart::{
    collect, device_to_value, parse_attr_row, parse_device_output, parse_scan, register,
};

const SCAN_FIXTURE: &str = "\
/dev/sda -d scsi # /dev/sda, SCSI device
/dev/nvme0 -d nvme # /dev/nvme0, NVMe device
";

const ATA_FIXTURE: &str = "\
smartctl 7.4
Device Model:     SAMPLE SSD 1TB
Serial Number:    ABC123
Rotation Rate:    Solid State Device
SMART Attributes Data Structure revision number: 1
ID# ATTRIBUTE_NAME          FLAG     VALUE WORST THRESH TYPE      UPDATED  WHEN_FAILED RAW_VALUE
  5 Reallocated_Sector_Ct   0x0033   100   100   010    Pre-fail  Always       -       0
  9 Power_On_Hours          0x0032   090   090   000    Old_age   Always       -       8765
194 Temperature_Celsius     0x0022   065   065   000    Old_age   Always       -       35
";

const NVME_FIXTURE: &str = "\
smartctl 7.4
Model Number:                       SAMPLE NVME
Serial Number:                      XYZ789
SMART/Health Information (NVMe Log 0x02)
Critical Warning:                   0x00
Temperature:                        35 Celsius
Available Spare:                    100%
Percentage Used:                    2%
Data Units Read:                    1,234 [633 MB]
Power Cycles:                       42
";

#[test]
fn parse_scan_lists_devices_with_types() {
    let devs = parse_scan(SCAN_FIXTURE);
    assert_eq!(
        devs,
        vec![
            ("/dev/sda".to_string(), "scsi".to_string()),
            ("/dev/nvme0".to_string(), "nvme".to_string()),
        ]
    );
}

#[test]
fn parse_scan_skips_comments_and_blanks() {
    assert!(parse_scan("# nothing here\n\n").is_empty());
}

#[test]
fn parse_attr_row_reads_columns() {
    let a = parse_attr_row(
        "  5 Reallocated_Sector_Ct   0x0033   100   100   010    Pre-fail  Always       -       0",
    )
    .expect("must parse");
    assert_eq!(a.num, 5);
    assert_eq!(a.name, "Reallocated_Sector_Ct");
    assert_eq!(a.value, 100);
    assert_eq!(a.threshold, 10);
    assert_eq!(a.raw, "0");
    assert!(parse_attr_row("ID# ATTRIBUTE_NAME FLAG").is_none());
}

#[test]
fn parse_ata_device_output() {
    let d = parse_device_output("/dev/sda", "scsi", ATA_FIXTURE);
    assert_eq!(d.model, "SAMPLE SSD 1TB");
    assert_eq!(d.serial, "ABC123");
    assert_eq!(d.protocol, "ata");
    assert_eq!(d.attributes.len(), 3);
    assert_eq!(d.attributes[0].name, "Reallocated_Sector_Ct");
}

#[test]
fn parse_nvme_device_output() {
    let d = parse_device_output("/dev/nvme0", "nvme", NVME_FIXTURE);
    assert_eq!(d.protocol, "nvme");
    assert!(!d.nvme.is_empty());
    let temp = d.nvme.iter().find(|(k, _)| k == "Temperature");
    assert_eq!(temp.map(|(_, v)| v.as_str()), Some("35 Celsius"));
}

#[test]
fn device_to_value_keys_by_device_name() {
    let d = parse_device_output("/dev/sda", "scsi", ATA_FIXTURE);
    let v = device_to_value(&d);
    let obj = v.as_object().expect("object");
    assert_eq!(
        obj.get("DeviceName").and_then(|v| v.as_str()),
        Some("/dev/sda SAMPLE SSD 1TB")
    );
    for k in ["model", "serial", "protocol", "attributes", "nvme"] {
        assert!(obj.contains_key(k), "missing key {}", k);
    }
}

#[test]
fn collect_never_panics_without_binary() {
    // No assertion on contents (environment-dependent) — must not panic.
    let _ = collect();
}

#[test]
fn register_plugin_appears_in_stats() {
    let s = crate::core::stats::GlancesStats::new(1.0);
    register(&s);
    assert!(s.plugin_names().contains(&"smart"));
}
