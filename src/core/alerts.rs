//! Alert engine: limit lookups, measurement classification, defaults.
//!
//! Each plugin carries a limits table (built-in defaults overridden by
//! config). Measurements classify into trigger words — DEFAULT, OK,
//! CAREFUL, WARNING, CRITICAL — optionally with event logging.

use super::events::{Event, EventLog};
use super::plugin::GlancesPluginModel;
use super::threshold::Severity;

/// A configured limit: a plain number or a raw string list (action
/// commands and flags arrive as lists).
#[derive(Debug, Clone, PartialEq)]
pub enum LimitValue {
    Float(f64),
    List(Vec<String>),
}

impl GlancesPluginModel {
    /// Read a limit as a number (lists parse their first item).
    fn limit_number(&self, key: &str) -> Option<f64> {
        match self.limits.get(key) {
            Some(LimitValue::Float(f)) => Some(*f),
            Some(LimitValue::List(l)) => l.first()?.parse::<f64>().ok(),
            None => None,
        }
    }

    /// Threshold for a severity: `<stat>_<level>` first, then
    /// `<plugin>_<level>`.
    pub fn get_limit(&self, severity: &str, stat_name: &str) -> Option<f64> {
        let stat = stat_name.to_lowercase();
        self.limit_number(&format!("{stat}_{severity}"))
            .or_else(|| self.limit_number(&format!("{}_{severity}", self.plugin_name)))
    }

    /// Action commands for a trigger plus whether they repeat. Stat-level
    /// entries win over plugin-level ones; nothing configured yields
    /// `(None, false)`.
    pub fn get_limit_action(&self, criticality: &str, stat_name: &str) -> (Option<Vec<String>>, bool) {
        let sev = criticality.to_lowercase();
        let stat = stat_name.to_lowercase();
        let plugin = self.plugin_name;
        for (key, repeat) in [
            (format!("{stat}_{sev}_action"), false),
            (format!("{stat}_{sev}_action_repeat"), true),
            (format!("{plugin}_{sev}_action"), false),
            (format!("{plugin}_{sev}_action_repeat"), true),
        ] {
            if let Some(found) = self.limits.get(&key) {
                let cmds = match found {
                    LimitValue::List(l) => l.clone(),
                    LimitValue::Float(f) => vec![format!("{f}")],
                };
                return (Some(cmds), repeat);
            }
        }
        (None, false)
    }

    /// Whether a stat logs its triggers: `<stat>_log`, else
    /// `<plugin>_log`, else the caller's default.
    pub fn get_limit_log(&self, stat_name: &str, default: bool) -> bool {
        let stat = stat_name.to_lowercase();
        [format!("{stat}_log"), format!("{}_log", self.plugin_name)]
            .into_iter()
            .filter_map(|key| match self.limits.get(&key) {
                Some(LimitValue::List(l)) => l.first().map(|s| s.to_lowercase() == "true"),
                Some(LimitValue::Float(f)) => Some(*f != 0.0),
                None => None,
            })
            .next()
            .unwrap_or(default)
    }

    /// Classify a measurement into a trigger word.
    ///
    /// The value is percent-normalized against `maximum`. Zero readings
    /// (unless highlighted), zero maximums, and non-finite results all
    /// yield DEFAULT. Otherwise the first reached band in
    /// critical→warning→careful order wins; with no bands configured the
    /// answer is DEFAULT, and a value under every band is OK. A current
    /// below `minimum` forces CAREFUL. When the log tag is set the word
    /// gains a `_LOG` suffix and the crossing is recorded. The trigger
    /// is always remembered per stat.
    #[allow(clippy::too_many_arguments)]
    pub fn get_alert(
        &mut self,
        current: f64,
        minimum: f64,
        maximum: f64,
        header: &str,
        action_key: Option<&str>,
        is_max: bool,
        highlight_zero: bool,
        log: Option<bool>,
        events: Option<&mut EventLog>,
    ) -> String {
        let _ = is_max;
        if !highlight_zero && current == 0.0 {
            return "DEFAULT".into();
        }
        if maximum == 0.0 {
            return "DEFAULT".into();
        }
        let pct = current * 100.0 / maximum;
        if !pct.is_finite() {
            return "DEFAULT".into();
        }
        let stat = stat_name(self.plugin_name, action_key, header);
        let bands = [
            (self.get_limit("critical", &stat), "CRITICAL"),
            (self.get_limit("warning", &stat), "WARNING"),
            (self.get_limit("careful", &stat), "CAREFUL"),
        ];
        let mut word = bands
            .iter()
            .find_map(|(limit, name)| match limit {
                Some(t) if pct >= *t => Some(*name),
                _ => None,
            })
            .unwrap_or(if bands_configured(&bands) { "OK" } else { "DEFAULT" });
        if current < minimum {
            word = "CAREFUL";
        }
        let mut out = word.to_string();
        if self.get_limit_log(&stat, log.unwrap_or(false)) && word != "DEFAULT" {
            out.push_str("_LOG");
            if let (Some(log), Some(sev)) = (events, trigger_severity(word)) {
                log.push(Event {
                    severity: sev,
                    stat: stat.clone(),
                    value: pct,
                    timestamp: std::time::SystemTime::now(),
                });
            }
        }
        self.thresholds.insert(stat, word.to_string());
        out
    }

    /// Classify with logging forced on.
    pub fn get_alert_log(
        &mut self,
        current: f64,
        maximum: f64,
        header: &str,
        events: Option<&mut EventLog>,
    ) -> String {
        self.get_alert(current, 0.0, maximum, header, None, false, false, Some(true), events)
    }

    /// Install built-in careful/warning/critical defaults under
    /// plugin-prefixed keys, leaving configured values untouched. The
    /// cpu ctx_switches row scales with logical core count instead of
    /// using the table values.
    pub fn apply_default_limits(&mut self, entries: &[(&str, f64, f64, f64)], ncpu: u64) {
        for (header, careful, warning, critical) in entries {
            let stem = if header.is_empty() {
                format!("{}_", self.plugin_name)
            } else {
                format!("{}_{}_", self.plugin_name, header)
            };
            let (c, w, t) = if *header == "ctx_switches" && self.plugin_name == "cpu" {
                let full = 500_000.0 * 0.10 * ncpu.max(1) as f64;
                (full * 0.80, full * 0.90, full)
            } else {
                (*careful, *warning, *critical)
            };
            for (kind, v) in [("careful", c), ("warning", w), ("critical", t)] {
                self.limits.entry(format!("{stem}{kind}")).or_insert(LimitValue::Float(v));
            }
        }
    }
}

/// plugin[_action_key][_header], lowercased; empty parts are skipped.
fn stat_name(plugin: &str, action_key: Option<&str>, header: &str) -> String {
    let mut parts = vec![plugin.to_string()];
    if let Some(ak) = action_key.filter(|s| !s.is_empty()) {
        parts.push(ak.to_string());
    }
    if !header.is_empty() {
        parts.push(header.to_string());
    }
    parts.join("_").to_lowercase()
}

fn bands_configured(bands: &[(Option<f64>, &str)]) -> bool {
    bands.iter().any(|(limit, _)| limit.is_some())
}

/// Severity carried by a trigger word; DEFAULT/OK/MAX carry none.
fn trigger_severity(word: &str) -> Option<Severity> {
    match word {
        "CAREFUL" => Some(Severity::Careful),
        "WARNING" => Some(Severity::Warning),
        "CRITICAL" => Some(Severity::Critical),
        _ => None,
    }
}

/// Built-in (careful, warning, critical) defaults per plugin, keyed by
/// header (`""` = the plugin-wide row). The cpu ctx_switches row holds
/// placeholders — real values scale with core count at install time.
pub fn default_limit_entries(plugin: &str) -> Vec<(&'static str, f64, f64, f64)> {
    let std = |h: &'static str| (h, 50.0, 70.0, 90.0);
    match plugin {
        "quicklook" => vec![std("cpu"), std("mem"), std("swap")],
        "cpu" => vec![
            std("user"),
            std("system"),
            std("steal"),
            std("iowait"),
            ("ctx_switches", f64::NAN, f64::NAN, f64::NAN),
        ],
        "percpu" => vec![std("user"), std("system")],
        "load" => vec![("", 0.7, 1.0, 5.0)],
        "mem" | "memswap" | "fs" => vec![std("")],
        "network" => vec![std("rx"), std("tx")],
        "sensors" => vec![("temperature_hdd", 45.0, 52.0, 60.0), ("battery", 70.0, 80.0, 90.0)],
        "processlist" => vec![std("cpu"), std("mem")],
        _ => Vec::new(),
    }
}
