//! /sys/class/hwmon reader — temperature, fan, voltage sensors.
//!
//! Each hwmon device lives at /sys/class/hwmon/hwmonN/ with files like
//!   temp1_input (millidegrees C)
//!   fan1_input (RPM)
//!   in0_input (millivolts)
//!   name (chip name)
//!
//! Linux exposes /sys/class/thermal/thermal_zone*/temp as a fallback for
//! CPU temperatures when hwmon isn't available.

use std::fs;
use std::path::Path;

use crate::core::error::{GlancesError, Result};

#[derive(Debug, Default, Clone)]
pub struct HwmonSensor {
    pub chip: String,
    pub label: String,
    pub value: f64,
    pub kind: SensorKind,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SensorKind { #[default] Temperature, Fan, Voltage }

impl SensorKind {
    pub fn suffix(self) -> &'static str {
        match self {
            SensorKind::Temperature => "input",
            SensorKind::Fan => "input",
            SensorKind::Voltage => "input",
        }
    }
}

pub fn read_all() -> Result<Vec<HwmonSensor>> {
    let mut out = Vec::new();
    let entries = match fs::read_dir("/sys/class/hwmon") {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(GlancesError::Io(e)),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let chip = fs::read_to_string(path.join("name"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| entry.file_name().to_string_lossy().into_owned());
        collect_from_dir(&path, &chip, &mut out);
    }
    Ok(out)
}

fn collect_from_dir(dir: &Path, chip: &str, out: &mut Vec<HwmonSensor>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Labels live beside the input file as `<stem>_label`
        // (e.g. temp1_label for temp1_input), not inside it.
        let label_path = dir.join(format!("{}_label", name.trim_end_matches("_input")));
        let label = match fs::read_to_string(&label_path) {
            Ok(s) => s.trim().to_string(),
            Err(_) => name.trim_end_matches("_input").to_string(),
        };
        if name.starts_with("temp") && name.ends_with("_input") {
            if let Ok(s) = fs::read_to_string(entry.path())
                && let Ok(milli) = s.trim().parse::<i64>() {
                    out.push(HwmonSensor { chip: chip.to_string(), label, value: milli as f64 / 1000.0, kind: SensorKind::Temperature });
                }
        } else if name.starts_with("fan") && name.ends_with("_input") {
            if let Ok(s) = fs::read_to_string(entry.path())
                && let Ok(rpm) = s.trim().parse::<i64>() {
                    out.push(HwmonSensor { chip: chip.to_string(), label, value: rpm as f64, kind: SensorKind::Fan });
                }
        } else if name.starts_with("in") && name.ends_with("_input")
            && let Ok(s) = fs::read_to_string(entry.path())
                && let Ok(mv) = s.trim().parse::<i64>() {
                    out.push(HwmonSensor { chip: chip.to_string(), label, value: mv as f64 / 1000.0, kind: SensorKind::Voltage });
                }
    }
}
