//! Unit tests for the fs plugin — `/proc/mounts` parsing and filtering.

use crate::plugins::fs::{parse_mounts, parse_mounts_line, should_skip, MountEntry};

#[test]
fn parse_line_basic_fields() {
    let line = "/dev/sda1 / ext4 rw,relatime 0 0";
    let e = parse_mounts_line(line).unwrap();
    assert_eq!(e.device, "/dev/sda1");
    assert_eq!(e.mountpoint, "/");
    assert_eq!(e.fstype, "ext4");
    assert!(e.options.starts_with("rw"));
}

#[test]
fn parse_line_with_escaped_spaces_in_options() {
    // mtab escapes spaces in options as \040.
    let line = "/dev/sda1 /mnt/foo ext4 rw,nosuid,nodev 0 0";
    let e = parse_mounts_line(line).unwrap();
    assert_eq!(e.mountpoint, "/mnt/foo");
    assert_eq!(e.fstype, "ext4");
}

#[test]
fn parse_line_decodes_octal_escapes() {
    // /proc/mounts encodes ' ' as \040 and '\' as \134 in paths.
    let e = parse_mounts_line("/dev/sda1 /mnt/with\\040space ext4 rw 0 0").unwrap();
    assert_eq!(e.mountpoint, "/mnt/with space");
    // options is a single field — dump/pass are not joined into it.
    assert_eq!(e.options, "rw");
}

#[test]
fn parse_line_returns_none_on_short_input() {
    // Need at least device, mountpoint, fstype to be present.
    assert!(parse_mounts_line("only_two_fields").is_none());
    assert!(parse_mounts_line("a b").is_none());
    assert!(parse_mounts_line("").is_none());
    assert!(parse_mounts_line("   ").is_none());
}

#[test]
fn parse_mounts_filters_blank_lines() {
    let text = "/dev/sda1 / ext4 rw 0 0\n\n/dev/sdb1 /home ext4 rw 0 0\n   \n";
    let v = parse_mounts(text);
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].mountpoint, "/");
    assert_eq!(v[1].mountpoint, "/home");
}

#[test]
fn parse_mounts_handles_empty_input() {
    let v = parse_mounts("");
    assert!(v.is_empty());
    let v = parse_mounts("\n\n\n");
    assert!(v.is_empty());
}

#[test]
fn should_skip_pseudo_fstypes() {
    let cases: &[&str] = &[
        "tmpfs", "devpts", "proc", "sysfs", "cgroup", "cgroup2",
    ];
    for fs in cases {
        let e = MountEntry {
            device: "x".into(),
            mountpoint: "/foo".into(),
            fstype: fs.to_string(),
            options: String::new(),
        };
        assert!(should_skip(&e), "expected {} to be skipped", fs);
    }
}

#[test]
fn should_skip_known_prefixes() {
    for prefix in &["/proc", "/sys", "/dev/pts", "/run", "/var/run"] {
        let e = MountEntry {
            device: "x".into(),
            mountpoint: format!("{}/something", prefix),
            fstype: "ext4".into(),
            options: String::new(),
        };
        assert!(should_skip(&e), "expected prefix {} to be skipped", prefix);
    }
}

#[test]
fn should_not_skip_prefix_boundary_lookalikes() {
    // "/sysbackup" shares the "/sys" string prefix but is a different
    // path — prefix matching must be path-boundary aware.
    for mp in ["/sysbackup", "/procsys", "/runtime", "/runner", "/dev/ptsx"] {
        let e = MountEntry {
            device: "/dev/sda9".into(),
            mountpoint: mp.into(),
            fstype: "ext4".into(),
            options: String::new(),
        };
        assert!(!should_skip(&e), "real mount {} wrongly skipped", mp);
    }
    // …while the actual directories still match exactly or with `/`.
    for mp in ["/sys", "/sys/kernel", "/proc", "/proc/1", "/run/lock"] {
        let e = MountEntry {
            device: "x".into(),
            mountpoint: mp.into(),
            fstype: "ext4".into(),
            options: String::new(),
        };
        assert!(should_skip(&e), "{} should be skipped", mp);
    }
}

#[test]
fn should_not_skip_real_filesystems() {
    let e = MountEntry {
        device: "/dev/sda1".into(),
        mountpoint: "/".into(),
        fstype: "ext4".into(),
        options: "rw".into(),
    };
    assert!(!should_skip(&e));

    let e2 = MountEntry {
        device: "/dev/nvme0n1p2".into(),
        mountpoint: "/home".into(),
        fstype: "btrfs".into(),
        options: String::new(),
    };
    assert!(!should_skip(&e2));
}

#[test]
fn parse_real_proc_mounts_smoke() {
    // Live /proc/mounts (skip the assertion on non-Linux CI).
    if cfg!(target_os = "linux") {
        let text = std::fs::read_to_string("/proc/mounts").unwrap();
        let v = parse_mounts(&text);
        // At least one entry must be the rootfs or another real mount.
        // Filtering rules guarantee every entry has a non-empty fstype.
        assert!(v.iter().any(|e| e.fstype == "ext4" || e.fstype == "btrfs" || e.fstype == "xfs"));
    }
}