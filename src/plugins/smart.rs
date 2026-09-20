//! S.M.A.R.T. disk health — per-device attributes via `smartctl`.
//!
//! Mirrors `glances/plugins/smart/__init__.py` (pySMART backend).
//! Linux-only. Each refresh runs `smartctl --scan` to enumerate devices
//! then `smartctl -a` per device, argv-only with no shell (same pattern
//! as `core/actions.rs`). Missing binary, missing permissions, or parse
//! failures yield an empty list — never an error.
//!
//! Stats are one object per device keyed by `DeviceName`
//! (`"<device> <model>"`): ATA devices carry an `attributes` table
//! (num/name/value/worst/threshold/type/raw), NVMe devices carry an
//! `nvme` health map parsed from log page 0x02.

use std::collections::BTreeMap;
use std::process::Command;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "smart";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(SmartPlugin::new()));
}

pub struct SmartPlugin {
    base: GlancesPluginModel,
}

impl SmartPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
        }
    }
}

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
pub fn smartctl_bin() -> Option<String> {
    for dir in ["/usr/sbin", "/usr/bin", "/sbin", "/bin"] {
        let full = format!("{}/smartctl", dir);
        if std::path::Path::new(&full).is_file() {
            return Some(full);
        }
    }
    None
}

/// Run `smartctl` argv-only, returning stdout on success.
fn run_smartctl(bin: &str, args: &[&str]) -> Option<String> {
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

/// Enumerate devices and read their attributes. Empty on any failure.
pub fn collect() -> Vec<SmartDevice> {
    let bin = match smartctl_bin() {
        Some(b) => b,
        None => return Vec::new(),
    };
    let scan = match run_smartctl(&bin, &["--scan"]) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for (device, typ) in parse_scan(&scan) {
        let text = match run_smartctl(&bin, &["-a", "-d", &typ, &device]) {
            Some(t) => t,
            None => continue,
        };
        out.push(parse_device_output(&device, &typ, &text));
    }
    out
}

/// Render one device as a stats object keyed by `DeviceName`.
pub fn device_to_value(d: &SmartDevice) -> Value {
    let mut obj = BTreeMap::new();
    let name = if d.model.is_empty() {
        d.device.clone()
    } else {
        format!("{} {}", d.device, d.model)
    };
    obj.insert("DeviceName".into(), Value::String(name));
    obj.insert("model".into(), Value::String(d.model.clone()));
    obj.insert("serial".into(), Value::String(d.serial.clone()));
    obj.insert("protocol".into(), Value::String(d.protocol.clone()));
    obj.insert(
        "attributes".into(),
        Value::Array(
            d.attributes
                .iter()
                .map(|a| {
                    let mut m = BTreeMap::new();
                    m.insert("num".into(), Value::Uint(a.num));
                    m.insert("name".into(), Value::String(a.name.clone()));
                    m.insert("value".into(), Value::Uint(a.value));
                    m.insert("worst".into(), Value::Uint(a.worst));
                    m.insert("threshold".into(), Value::Uint(a.threshold));
                    m.insert("type".into(), Value::String(a.attr_type.clone()));
                    m.insert("raw".into(), Value::String(a.raw.clone()));
                    Value::Object(m)
                })
                .collect(),
        ),
    );
    obj.insert(
        "nvme".into(),
        Value::Array(
            d.nvme
                .iter()
                .map(|(k, v)| {
                    let mut m = BTreeMap::new();
                    m.insert("name".into(), Value::String(k.clone()));
                    m.insert("value".into(), Value::String(v.clone()));
                    Value::Object(m)
                })
                .collect(),
        ),
    );
    Value::Object(obj)
}

impl Plugin for SmartPlugin {
    fn name(&self) -> &'static str {
        NAME
    }
    fn reset(&mut self) {
        self.base.reset();
    }
    fn stats(&self) -> &Value {
        &self.base.stats
    }
    fn model(&self) -> Option<&GlancesPluginModel> {
        Some(&self.base)
    }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> {
        Some(&mut self.base)
    }
    fn stats_mut(&mut self) -> &mut Value {
        &mut self.base.stats
    }
    fn get_key(&self) -> Option<&'static str> {
        Some("DeviceName")
    }

    fn update(&mut self) -> Result<()> {
        if !cfg!(target_os = "linux") {
            self.base.stats = Value::Array(Vec::new());
            return Ok(());
        }
        let devices = collect();
        self.base.stats = Value::Array(devices.iter().map(device_to_value).collect());
        Ok(())
    }
}
