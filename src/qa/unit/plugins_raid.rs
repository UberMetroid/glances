//! Unit tests for the raid plugin — /proc/mdstat parsing.

use crate::plugins::raid::{
    count_failed_from_state, entry_to_value, parse_header, parse_mdstat,
    parse_status_line,
};

#[test]
fn parse_header_extracts_name_status_level_components() {
    let line = "md0 : active raid1 sda1[0] sdb1[1]";
    let e = parse_header(line).expect("header");
    assert_eq!(e.name, "md0");
    assert_eq!(e.status, "active");
    assert_eq!(e.level, "raid1");
    assert_eq!(e.components, vec!["sda1[0]", "sdb1[1]"]);
}

#[test]
fn parse_header_returns_none_for_non_md_line() {
    assert!(parse_header("Personalities : [raid1]").is_none());
    assert!(parse_header("unused devices: <none>").is_none());
    assert!(parse_header("").is_none());
    assert!(parse_header("random text").is_none());
}

#[test]
fn parse_header_handles_raid5_with_three_components() {
    let line = "md1 : active raid5 sdc1[0] sdd1[3] sde1[4]";
    let e = parse_header(line).expect("header");
    assert_eq!(e.name, "md1");
    assert_eq!(e.level, "raid5");
    assert_eq!(e.components.len(), 3);
}

#[test]
fn parse_status_line_extracts_total_and_working() {
    let line = "1953511936 blocks super 1.2 [2/2] [UU]";
    let (total, working) = parse_status_line(line).unwrap();
    assert_eq!(total, 2);
    assert_eq!(working, 2);
}

#[test]
fn parse_status_line_handles_degraded_array() {
    let line = "1953511936 blocks super 1.2 [2/1] [U_]";
    let (total, working) = parse_status_line(line).unwrap();
    assert_eq!(total, 2);
    assert_eq!(working, 1);
}

#[test]
fn count_failed_from_state_counts_non_u_chars() {
    assert_eq!(count_failed_from_state("... [2/2] [UU]"), 0);
    assert_eq!(count_failed_from_state("... [2/1] [U_]"), 1);
    assert_eq!(count_failed_from_state("... [3/1] [U__]"), 2);
    // Bracket with 'F' for faulty device.
    assert_eq!(count_failed_from_state("... [3/2] [UFU]"), 1);
}

#[test]
fn count_failed_returns_zero_on_no_brackets() {
    assert_eq!(count_failed_from_state("no brackets here"), 0);
    assert_eq!(count_failed_from_state(""), 0);
}

#[test]
fn parse_mdstat_parses_full_example() {
    let text = "\
Personalities : [raid1] [raid5]
md0 : active raid1 sda1[0] sdb1[1]
      1953511936 blocks super 1.2 [2/2] [UU]

md1 : active raid5 sdc1[0] sdd1[1] sde1[2]
      3907024128 blocks super 1.2 level 5 [3/3] [UUU]
unused devices: <none>
";
    let entries = parse_mdstat(text);
    assert_eq!(entries.len(), 2);
    let md0 = &entries[0];
    assert_eq!(md0.name, "md0");
    assert_eq!(md0.status, "active");
    assert_eq!(md0.level, "raid1");
    assert_eq!(md0.total_devices, 2);
    assert_eq!(md0.working_devices, 2);
    assert_eq!(md0.failed_devices, 0);
    assert_eq!(md0.components, vec!["sda1[0]", "sdb1[1]"]);
    let md1 = &entries[1];
    assert_eq!(md1.level, "raid5");
    assert_eq!(md1.total_devices, 3);
    assert_eq!(md1.working_devices, 3);
    assert_eq!(md1.components.len(), 3);
}

#[test]
fn parse_mdstat_empty_input_returns_empty_vec() {
    assert!(parse_mdstat("").is_empty());
    assert!(parse_mdstat("Personalities : [raid1]\nunused devices: <none>\n").is_empty());
}

#[test]
fn parse_mdstat_handles_degraded_status() {
    let text = "\
md0 : active raid1 sda1[0] sdb1[2]
      1953511936 blocks super 1.2 [2/1] [U_]
";
    let entries = parse_mdstat(text);
    assert_eq!(entries.len(), 1);
    let md0 = &entries[0];
    assert_eq!(md0.working_devices, 1);
    assert_eq!(md0.failed_devices, 1);
}

#[test]
fn parse_mdstat_handles_missing_status_line() {
    // Edge: header only, no totals line (kernel mid-update).
    let text = "md0 : active raid1 sda1[0]\n";
    let entries = parse_mdstat(text);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].total_devices, 0);
    assert_eq!(entries[0].working_devices, 0);
}

#[test]
fn parse_mdstat_progress_and_bitmap_lines_dont_fabricate_failures() {
    // Regression: the `[====>...]` progress bar and `[0KB]` bitmap
    // brackets used to be counted as failed-device state chars.
    let text = "\
md0 : active raid1 sda1[0] sdb1[1]
      1953511936 blocks super 1.2 [2/2] [UU]
      [====>................]  resync = 20.0% (390713344/1953511936) finish=15.6min speed=102041K/sec
      bitmap: 0/15 pages [0KB], 65536KB chunk
";
    let entries = parse_mdstat(text);
    assert_eq!(entries.len(), 1);
    let md0 = &entries[0];
    assert_eq!(md0.total_devices, 2);
    assert_eq!(md0.working_devices, 2);
    assert_eq!(md0.failed_devices, 0,
        "progress bar / bitmap brackets must not count as failed devices");
}

#[test]
fn entry_to_value_has_all_required_keys() {
    let e = crate::plugins::raid::MdEntry {
        name: "md0".into(),
        status: "active".into(),
        level: "raid1".into(),
        total_devices: 2,
        working_devices: 2,
        failed_devices: 0,
        components: vec!["sda1[0]".into(), "sdb1[1]".into()],
    };
    let v = entry_to_value(&e);
    let obj = v.as_object().expect("object");
    assert_eq!(obj.get("raid_name").and_then(|x| x.as_str()), Some("md0"));
    assert_eq!(obj.get("status").and_then(|x| x.as_str()), Some("active"));
    assert_eq!(obj.get("level").and_then(|x| x.as_str()), Some("raid1"));
    assert_eq!(obj.get("total").and_then(|x| x.as_f64()), Some(2.0));
    assert_eq!(obj.get("working").and_then(|x| x.as_f64()), Some(2.0));
    assert_eq!(obj.get("failed").and_then(|x| x.as_f64()), Some(0.0));
    let comps = obj.get("components").and_then(|x| x.as_array()).expect("components array");
    assert_eq!(comps.len(), 2);
}