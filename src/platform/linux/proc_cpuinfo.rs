//! /proc/cpuinfo reader — CPU model name for the `cpu_name` field.

use std::fs;

/// First `model name` entry in /proc/cpuinfo (e.g. "Intel(R) Core(TM)
/// i7-9700K CPU @ 3.60GHz"). `None` on read failure or missing key.
pub fn model_name() -> Option<String> {
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    parse_model_name(&text)
}

/// Number of logical CPUs (`processor` entries). Falls back to the
/// `cpuN` rows in /proc/stat if /proc/cpuinfo can't be read — never 0
/// on a healthy Linux system.
pub fn cpu_count() -> usize {
    let n = fs::read_to_string("/proc/cpuinfo")
        .map(|t| t.lines().filter(|l| l.starts_with("processor")).count())
        .unwrap_or(0);
    if n > 0 { return n; }
    crate::platform::linux::proc_stat::read()
        .map(|s| s.per_cpu.len().max(1))
        .unwrap_or(1)
}

pub fn parse_model_name(text: &str) -> Option<String> {
    for line in text.lines() {
        let mut parts = line.splitn(2, ':');
        if parts.next()?.trim() == "model name" {
            let name = parts.next()?.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_first_model_name() {
        let text = "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Intel(R) Core(TM) i7-9700K CPU @ 3.60GHz\nstepping\t: 13\n";
        assert_eq!(
            parse_model_name(text).as_deref(),
            Some("Intel(R) Core(TM) i7-9700K CPU @ 3.60GHz")
        );
    }

    #[test]
    fn missing_key_returns_none() {
        assert_eq!(parse_model_name("processor\t: 0\n"), None);
    }

    #[test]
    fn empty_value_returns_none() {
        assert_eq!(parse_model_name("model name\t:   \n"), None);
    }
}
