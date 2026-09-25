//! Severity vocabulary: four ordered alert levels.
//!
//! Levels rank Ok < Careful < Warning < Critical. Values are classified
//! against per-stat thresholds, and limits are looked up stat-first,
//! plugin-second.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Ok = 0,
    Careful = 1,
    Warning = 2,
    Critical = 3,
}

/// Classify a percentage against the bands that are set.
///
/// Returns the highest level whose threshold the value reaches. Bands
/// left unset are skipped; with nothing set the answer is Ok.
pub fn evaluate(value_pct: f64, careful: Option<f64>, warning: Option<f64>, critical: Option<f64>) -> Severity {
    let bands = [(careful, Severity::Careful), (warning, Severity::Warning), (critical, Severity::Critical)];
    let mut level = Severity::Ok;
    for (limit, sev) in bands {
        if limit.is_some_and(|t| value_pct >= t) {
            level = sev;
        }
    }
    level
}

/// Find a threshold: `<stat>_<level>` wins, `<plugin>_<level>` is the
/// fallback. `stat` is the qualified name (`cpu_user`), `plugin` the
/// bare one (`cpu`). The Ok level always resolves to 0.0.
pub fn get_limit(
    stat: &str,
    plugin: &str,
    severity: Severity,
    plugin_limits: &std::collections::HashMap<String, f64>,
) -> Option<f64> {
    let word = match severity {
        Severity::Ok => return Some(0.0),
        Severity::Careful => "careful",
        Severity::Warning => "warning",
        Severity::Critical => "critical",
    };
    plugin_limits
        .get(&format!("{stat}_{word}"))
        .or_else(|| plugin_limits.get(&format!("{plugin}_{word}")))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels_rank_ok_to_critical() {
        assert!(Severity::Ok < Severity::Careful);
        assert!(Severity::Careful < Severity::Warning);
        assert!(Severity::Warning < Severity::Critical);
    }
    #[test]
    fn boundary_value_reaches_the_band() {
        assert_eq!(evaluate(70.0, Some(50.0), Some(70.0), Some(90.0)), Severity::Warning);
        assert_eq!(evaluate(69.9, Some(50.0), Some(70.0), Some(90.0)), Severity::Careful);
    }
    #[test]
    fn unset_bands_are_skipped() {
        assert_eq!(evaluate(99.0, None, None, None), Severity::Ok);
        assert_eq!(evaluate(99.0, None, Some(70.0), None), Severity::Warning);
    }
    #[test]
    fn stat_limit_beats_plugin_limit() {
        let limits = std::collections::HashMap::from([
            ("cpu_user_warning".to_string(), 60.0),
            ("cpu_warning".to_string(), 70.0),
        ]);
        assert_eq!(get_limit("cpu_user", "cpu", Severity::Warning, &limits), Some(60.0));
        assert_eq!(get_limit("cpu_idle", "cpu", Severity::Warning, &limits), Some(70.0));
        assert_eq!(get_limit("cpu_idle", "cpu", Severity::Critical, &limits), None);
    }
}
