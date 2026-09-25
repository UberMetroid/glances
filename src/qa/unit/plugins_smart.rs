//! Tests for the smart plugin — smartctl output parsers (fixture-based,
//! no live binary required).

use std::time::{Duration, Instant};

use crate::core::plugin::Plugin;
use crate::plugins::smart::{
    cache_fresh, collect, device_to_value, parse_attr_row, parse_device_output, parse_scan,
    register, SmartPlugin,
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

/// Write an executable `smartctl` stub answering `--scan` and `-a`
/// from the module fixtures. `fail` makes every call exit 1.
fn stub_smartctl(dir: &std::path::Path, fail: bool) {
    let body = if fail {
        "#!/bin/sh\nexit 1\n".to_string()
    } else {
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--scan\" ]; then\ncat <<'EOF'\n{SCAN_FIXTURE}EOF\nelif [ \"$3\" = \"nvme\" ]; then\ncat <<'EOF2'\n{NVME_FIXTURE}EOF2\nelse\ncat <<'EOF3'\n{ATA_FIXTURE}EOF3\nfi\n"
        )
    };
    let p = dir.join("smartctl");
    std::fs::write(&p, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn collect_end_to_end_through_stub_binary() {
    // Full pipeline: lookup -> --scan -> per-device -a -> parse.
    let tmp = crate::qa::harness::TempDir::new("smart-stub");
    stub_smartctl(tmp.path(), false);
    let _env = crate::qa::harness::HelperEnv::set(tmp.path());
    let devs = collect();
    assert_eq!(devs.len(), 2);
    assert_eq!(devs[0].device, "/dev/sda");
    assert_eq!(devs[0].model, "SAMPLE SSD 1TB");
    assert_eq!(devs[0].serial, "ABC123");
    assert_eq!(devs[0].attributes.len(), 3);
    assert_eq!(devs[1].device, "/dev/nvme0");
    assert_eq!(devs[1].model, "SAMPLE NVME");
    assert!(!devs[1].nvme.is_empty());
}

#[test]
fn collect_empty_when_helper_fails() {
    let tmp = crate::qa::harness::TempDir::new("smart-fail");
    stub_smartctl(tmp.path(), true);
    let _env = crate::qa::harness::HelperEnv::set(tmp.path());
    assert!(collect().is_empty());
}

#[test]
fn register_plugin_appears_in_stats() {
    let s = crate::core::stats::GlancesStats::new(1.0);
    register(&s);
    assert!(s.plugin_names().contains(&"smart"));
}

#[test]
fn cache_fresh_holds_for_sixty_seconds() {
    let now = Instant::now();
    assert!(!cache_fresh(None, now), "no sweep yet must re-collect");
    assert!(cache_fresh(Some(now), now));
    assert!(cache_fresh(Some(now - Duration::from_secs(59)), now));
    assert!(!cache_fresh(Some(now - Duration::from_secs(60)), now), "TTL edge is stale");
    assert!(!cache_fresh(Some(now - Duration::from_secs(61)), now));
}

#[test]
fn update_twice_is_stable_and_reset_clears() {
    let mut p = SmartPlugin::new();
    p.update().expect("update ok");
    let first = format!("{:?}", p.stats());
    p.update().expect("second update ok");
    assert_eq!(format!("{:?}", p.stats()), first, "cached tick must match");
    p.reset();
    assert!(p.stats().as_array().unwrap().is_empty());
}
