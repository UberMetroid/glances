//! Fs plugin behavior: mounts parsing, octal escapes, skip rules,
//! emitted key contract, and rootfs redirection.

use crate::plugins::fs::{parse_mounts, parse_mounts_line, should_skip, MountEntry};
use crate::plugins::fs_rootfs::{read_mounts_under, resolve_from, RootFs};
use crate::qa::harness::TempDir;

fn entry(device: &str, mountpoint: &str, fstype: &str) -> MountEntry {
    MountEntry {
        device: device.into(),
        mountpoint: mountpoint.into(),
        fstype: fstype.into(),
        options: String::new(),
    }
}

#[test]
fn line_fields_split_cleanly() {
    let e = parse_mounts_line("/dev/sda1 / ext4 rw,relatime 0 0").unwrap();
    assert_eq!((e.device.as_str(), e.mountpoint.as_str(), e.fstype.as_str()), ("/dev/sda1", "/", "ext4"));
    assert!(e.options.starts_with("rw"));
    assert_eq!(e.options, "rw,relatime");
}

#[test]
fn octal_escapes_decode_in_paths() {
    let e = parse_mounts_line("/dev/sda1 /mnt/with\\040space ext4 rw 0 0").unwrap();
    assert_eq!(e.mountpoint, "/mnt/with space");
    assert_eq!(e.options, "rw");
}

#[test]
fn short_lines_yield_nothing() {
    for bad in ["only_two_fields", "a b", "", "   "] {
        assert!(parse_mounts_line(bad).is_none(), "{bad:?} must not parse");
    }
}

#[test]
fn table_skips_blanks_and_keeps_order() {
    let v = parse_mounts("/dev/sda1 / ext4 rw 0 0\n\n/dev/sdb1 /home ext4 rw 0 0\n   \n");
    assert_eq!(v.len(), 2);
    assert_eq!((v[0].mountpoint.as_str(), v[1].mountpoint.as_str()), ("/", "/home"));
    assert!(parse_mounts("").is_empty());
    assert!(parse_mounts("\n\n\n").is_empty());
}

#[test]
fn pseudo_types_filter_out() {
    for fs in ["tmpfs", "devpts", "proc", "sysfs", "cgroup", "cgroup2", "ramfs", "debugfs"] {
        assert!(should_skip(&entry("x", "/foo", fs)), "{fs} must filter");
    }
}

#[test]
fn prefixes_match_on_path_boundaries() {
    for mp in ["/sysbackup", "/procsys", "/runtime", "/runner", "/dev/ptsx", "/", "/home"] {
        assert!(!should_skip(&entry("/dev/sda9", mp, "ext4")), "{mp} must survive");
    }
    for mp in ["/sys", "/sys/kernel", "/proc", "/proc/1", "/run/lock", "/dev/pts/0", "/var/run/x"] {
        assert!(should_skip(&entry("x", mp, "ext4")), "{mp} must filter");
    }
}

#[test]
fn live_mounts_table_parses() {
    let text = std::fs::read_to_string("/proc/mounts").unwrap();
    let v = parse_mounts(&text);
    assert!(v.iter().any(|e| ["ext4", "btrfs", "xfs", "overlay"].contains(&e.fstype.as_str())));
}

#[test]
fn emitted_rows_carry_the_key_contract() {
    use crate::core::plugin::Plugin;
    use crate::plugins::fs::FsPlugin;
    assert_eq!(FsPlugin::new().get_key(), Some("mnt_point"));
    let mut p = FsPlugin::new();
    p.update().expect("fs update ok");
    let arr = p.stats().as_array().expect("array stats");
    assert!(!arr.is_empty());
    for v in arr {
        let obj = v.as_object().expect("object row");
        for k in ["key", "device_name", "fs_type", "mnt_point", "options",
                  "size", "used", "free", "percent"] {
            assert!(obj.contains_key(k), "missing {k}");
        }
    }
}

#[test]
fn rootfs_resolution_defaults_and_host() {
    let unset = resolve_from(None);
    assert_eq!(unset.mounts_file.to_str().unwrap(), "/proc/mounts");
    assert_eq!(unset.prefix.to_str().unwrap(), "/");
    assert_eq!(resolve_from(Some("")), unset);
    assert_eq!(resolve_from(Some("/")), unset);
    let host = resolve_from(Some("/host"));
    assert_eq!(host.mounts_file.to_str().unwrap(), "/host/proc/1/mounts");
    assert_eq!(host.prefix.to_str().unwrap(), "/host");
}

#[test]
fn host_rootfs_strips_prefix_and_file_binds() {
    let dir = TempDir::new("fs-rootfs");
    let root = dir.path();
    std::fs::create_dir_all(root.join("proc/1")).unwrap();
    std::fs::create_dir_all(root.join("boot")).unwrap();
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/bin/nvidia-smi"), "fake").unwrap();
    std::fs::write(
        root.join("proc/1/mounts"),
        "/dev/sda3 / ext4 rw,relatime 0 0\n\
         /dev/sda2 /boot ext4 ro,relatime 0 0\n\
         overlay /usr/bin/nvidia-smi overlay ro,relatime 0 0\n\
         tmpfs /run tmpfs rw,nosuid 0 0\n",
    ).unwrap();
    let out = read_mounts_under(&RootFs {
        mounts_file: root.join("proc/1/mounts"),
        prefix: root.to_path_buf(),
    }).expect("fake rootfs reads");
    let mnts: Vec<String> = out.iter()
        .map(|v| v.as_object().unwrap()["mnt_point"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(mnts, vec!["/".to_string(), "/boot".to_string()]);
    for v in &out {
        let obj = v.as_object().unwrap();
        assert!(obj["size"].as_f64().unwrap() > 0.0, "statvfs ran: {obj:?}");
    }
}
