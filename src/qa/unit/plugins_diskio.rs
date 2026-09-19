//! Unit tests for the diskio plugin — disk name filtering and Value shape.

use crate::platform::linux::proc_diskstats::DiskStats;
use crate::plugins::diskio::{disk_to_value, is_partition, should_include};

fn disk(name: &str, reads: u64, sectors_read: u64, writes: u64, sectors_written: u64) -> DiskStats {
    DiskStats {
        name: name.to_string(),
        reads_completed: reads,
        sectors_read,
        writes_completed: writes,
        sectors_written,
    }
}

#[test]
fn should_include_keeps_whole_disks() {
    for n in &["sda", "sdb", "vda", "vdb", "nvme0n1", "xvda"] {
        assert!(should_include(n), "expected {} to be included", n);
    }
}

#[test]
fn should_include_excludes_partitions() {
    for n in &["sda1", "sda2", "vda3", "nvme0n1p1", "nvme0n1p5"] {
        assert!(!should_include(n), "expected {} to be filtered (partition)", n);
    }
}

#[test]
fn should_include_excludes_dm_and_md() {
    assert!(!should_include("dm-0"));
    assert!(!should_include("dm-1"));
    assert!(!should_include("md0"));
    assert!(!should_include("md127"));
    // md with longer names that happen to start with "md" — keep.
    assert!(should_include("mdfoo"));
}

#[test]
fn disk_to_value_has_all_required_fields() {
    let d = disk("sda", 1000, 2000, 500, 1000);
    let v = disk_to_value(&d);
    let obj = v.as_object().expect("object");
    assert!(obj.contains_key("disk_name"));
    assert!(obj.contains_key("read_count"));
    assert!(obj.contains_key("write_count"));
    assert!(obj.contains_key("read_bytes"));
    assert!(obj.contains_key("write_bytes"));
    assert_eq!(obj.get("disk_name").and_then(|x| x.as_str()), Some("sda"));
    assert_eq!(obj.get("read_count").and_then(|x| x.as_f64()), Some(1000.0));
    assert_eq!(obj.get("write_count").and_then(|x| x.as_f64()), Some(500.0));
    // 2000 sectors * 512 = 1,024,000 bytes
    assert_eq!(obj.get("read_bytes").and_then(|x| x.as_f64()), Some(1_024_000.0));
    assert_eq!(obj.get("write_bytes").and_then(|x| x.as_f64()), Some(512_000.0));
}

#[test]
fn disk_to_value_handles_zero_counts() {
    let d = disk("nvme0n1", 0, 0, 0, 0);
    let v = disk_to_value(&d);
    let obj = v.as_object().unwrap();
    assert_eq!(obj.get("read_count").and_then(|x| x.as_f64()), Some(0.0));
    assert_eq!(obj.get("read_bytes").and_then(|x| x.as_f64()), Some(0.0));
}

#[test]
fn is_partition_recognises_both_styles() {
    assert!(is_partition("sda1"));
    assert!(is_partition("sda10"));
    assert!(is_partition("nvme0n1p1"));
    assert!(is_partition("vdb99"));
    assert!(!is_partition("sda"));
    assert!(!is_partition("nvme0n1"));
    assert!(!is_partition("xvdb"));
}

#[test]
fn digit_suffixed_whole_disks_are_not_partitions() {
    // Regression: sr0/zram0/nbd0/rbd0/mmcblk0/loop0 are whole disks
    // whose names end in a digit — the bare letter-prefix heuristic
    // misclassified them as partitions.
    for n in ["sr0", "zram0", "nbd0", "rbd0", "mmcblk0", "loop0"] {
        assert!(!is_partition(n), "{} is a whole disk", n);
        assert!(should_include(n), "{} should be kept", n);
    }
    // Their partitions (p<N> form) are still detected.
    assert!(is_partition("nvme0n1p1"));
    assert!(is_partition("mmcblk0p1"));
    assert!(is_partition("nbd0p2"));
}

#[test]
fn fixture_parses_two_real_disks() {
    // The shared fixture used by proc_diskstats tests covers sda + nvme0n1.
    let txt = include_str!("../fixtures/proc/diskstats_normal.txt");
    let parsed = crate::platform::linux::proc_diskstats::parse(txt).unwrap();
    let mut included = Vec::new();
    for d in &parsed {
        if should_include(&d.name) { included.push(d.name.clone()); }
    }
    assert!(included.contains(&"sda".to_string()));
    assert!(included.contains(&"nvme0n1".to_string()));
    assert!(!included.iter().any(|n| n.starts_with("sda1")));
}