//! smartctl output parsers (fixture-testable, no binary needed).

use std::process::Command;

/// One ATA SMART attribute row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SmartAttr {
    pub num: u64,
    pub name: String,
    pub value: u64,
    pub worst: u64,
    pub threshold: u64,
    pub attr_type: String,
    pub raw: String,
}

/// One probed device with its parsed attributes/health.
#[derive(Debug, Clone, Default)]
pub struct SmartDevice {
    pub device: String,
    pub dev_type: String,
    pub model: String,
    pub serial: String,
    pub protocol: String,
    pub attributes: Vec<SmartAttr>,
    pub nvme: Vec<(String, String)>,
}

/// Locate the `smartctl` binary without a shell.
/// `GLANCES_HELPER_DIR`, when set, is searched first — a hook for
/// tests (stub binaries) and debugging; default lookup is unchanged.
pub fn smartctl_bin() -> Option<String> {
    if let Ok(dir) = std::env::var("GLANCES_HELPER_DIR") {
        let full = format!("{dir}/smartctl");
        if std::path::Path::new(&full).is_file() {
            return Some(full);
        }
    }
    for dir in ["/usr/sbin", "/usr/bin", "/sbin", "/bin"] {
        let full = format!("{}/smartctl", dir);
        if std::path::Path::new(&full).is_file() {
            return Some(full);
        }
    }
    None
}

/// Run `smartctl` argv-only, returning stdout on success.
pub(crate) fn run_smartctl(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Parse `smartctl --scan` output into (device, type) pairs.
/// Example: `/dev/sda -d scsi # /dev/sda, SCSI device`.
pub fn parse_scan(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Strip the trailing `# comment`.
        let head = match line.find(" #") {
            Some(i) => &line[..i],
            None => line,
        };
        let mut parts = head.split_whitespace();
        let dev = match parts.next() {
            Some(d) => d,
            None => continue,
        };
        let mut typ = String::new();
        let mut it = parts;
        while let Some(tok) = it.next() {
            if tok == "-d" {
                if let Some(t) = it.next() {
                    typ = t.to_string();
                }
                break;
            }
        }
        if typ.is_empty() {
            typ = "auto".to_string();
        }
        out.push((dev.to_string(), typ));
    }
    out
}

/// Parse one `smartctl -a` output into a device record.
pub fn parse_device_output(device: &str, dev_type: &str, text: &str) -> SmartDevice {
    let mut dev = SmartDevice {
        device: device.to_string(),
        dev_type: dev_type.to_string(),
        ..SmartDevice::default()
    };
    let mut in_attr_table = false;
    let mut in_nvme = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Device Model:") {
            dev.model = rest.trim().to_string();
        } else if let Some(rest) = trimmed.strip_prefix("Model Number:") {
            // NVMe form ("Model Number: ..."); ATA uses Device Model.
            dev.model = rest.trim().to_string();
        } else if let Some(rest) = trimmed.strip_prefix("Serial Number:") {
            dev.serial = rest.trim().to_string();
        } else if trimmed.starts_with("SMART/Health Information (NVMe") {
            in_nvme = true;
            in_attr_table = false;
            dev.protocol = "nvme".to_string();
        } else if trimmed.starts_with("ID#") && trimmed.contains("ATTRIBUTE_NAME") {
            in_attr_table = true;
            in_nvme = false;
            if dev.protocol.is_empty() {
                dev.protocol = "ata".to_string();
            }
        } else if in_attr_table {
            if trimmed.is_empty() {
                in_attr_table = false;
            } else if let Some(attr) = parse_attr_row(trimmed) {
                dev.attributes.push(attr);
            }
        } else if in_nvme {
            if trimmed.is_empty() {
                in_nvme = false;
            } else if let Some((k, v)) = trimmed.split_once(':') {
                dev.nvme.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }
    if dev.protocol.is_empty() {
        dev.protocol = if dev_type == "nvme" {
            "nvme".to_string()
        } else {
            "unknown".to_string()
        };
    }
    dev
}

/// Parse one ATA attribute row:
/// `  5 Reallocated_Sector_Ct   0x0033   100   100   010    Pre-fail  Always       -       0`
pub fn parse_attr_row(line: &str) -> Option<SmartAttr> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 10 {
        return None;
    }
    // First token must be the numeric attribute id.
    let num = parts[0].parse::<u64>().ok()?;
    Some(SmartAttr {
        num,
        name: parts[1].to_string(),
        value: parts[3].parse().unwrap_or(0),
        worst: parts[4].parse().unwrap_or(0),
        threshold: parts[5].parse().unwrap_or(0),
        attr_type: parts[6].to_string(),
        raw: parts[9..].join(" "),
    })
}
