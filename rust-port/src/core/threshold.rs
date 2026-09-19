//! Threshold severity classifier — mirrors `glances/thresholds.py`.
//!
//! Four levels (OK < CAREFUL < WARNING < CRITICAL), totally ordered, with
//! `get_limit()`-based lookup precedence `<stat>_<severity>` else
//! `<plugin>_<severity>` (per `model.py:964-978`).

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Ok = 0,
    Careful = 1,
    Warning = 2,
    Critical = 3,
}

impl PartialOrd for Severity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Severity {
    fn cmp(&self, other: &Self) -> Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

/// Compute the severity for a value against careful/warning/critical thresholds.
///
/// `value_pct` is the value expressed as a percent (0-100). Returns the highest
/// severity whose threshold the value exceeds. If no threshold is set, returns
/// `Severity::Ok`.
pub fn evaluate(value_pct: f64, careful: Option<f64>, warning: Option<f64>, critical: Option<f64>) -> Severity {
    let mut sev = Severity::Ok;
    if let Some(c) = careful { if value_pct >= c { sev = Severity::Careful; } }
    if let Some(w) = warning { if value_pct >= w { sev = Severity::Warning; } }
    if let Some(cr) = critical { if value_pct >= cr { sev = Severity::Critical; } }
    sev
}

/// Lookup precedence: `<plugin_stat>_<sev>` first, else `<plugin>_<sev>`.
/// `plugin_stat` is the fully-qualified stat name (e.g. `cpu_user`),
/// `plugin` is the bare plugin name (e.g. `cpu`).
pub fn get_limit(
    plugin_stat: &str,
    plugin: &str,
    severity: Severity,
    plugin_limits: &std::collections::HashMap<String, f64>,
) -> Option<f64> {
    let sev = match severity {
        Severity::Ok => return Some(0.0),
        Severity::Careful => "careful",
        Severity::Warning => "warning",
        Severity::Critical => "critical",
    };
    let stat_key = format!("{}_{}", plugin_stat, sev);
    if let Some(v) = plugin_limits.get(&stat_key) { return Some(*v); }
    let plugin_key = format!("{}_{}", plugin, sev);
    plugin_limits.get(&plugin_key).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordering() {
        assert!(Severity::Ok < Severity::Careful);
        assert!(Severity::Critical > Severity::Warning);
    }
    #[test]
    fn evaluate_below_all() {
        let s = evaluate(10.0, Some(50.0), Some(70.0), Some(90.0));
        assert_eq!(s, Severity::Ok);
    }
    #[test]
    fn evaluate_above_critical() {
        let s = evaluate(95.0, Some(50.0), Some(70.0), Some(90.0));
        assert_eq!(s, Severity::Critical);
    }
}
