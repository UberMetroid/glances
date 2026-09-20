//! Alert engine — get_alert/get_limit/views/default limits.
//! Mirrors the alert half of upstream GlancesPluginModel (model.py).

use super::events::{Event, EventLog};
use super::plugin::GlancesPluginModel;
use super::threshold::Severity;

impl GlancesPluginModel {
    /// Float value of a limit entry (`Float` as-is; `List` parses its
    /// first item — upstream stores every limit as a string).
    fn limit_float(&self, key: &str) -> Option<f64> {
        match self.limits.get(key) {
            Some(LimitValue::Float(f)) => Some(*f),
            Some(LimitValue::List(l)) => l.first()?.parse::<f64>().ok(),
            None => None,
        }
    }

    /// Limit lookup with `<stat>_<sev>`-before-`<plugin>_<sev>`
    /// precedence (upstream `get_limit` parity).
    pub fn get_limit(&self, severity: &str, stat_name: &str) -> Option<f64> {
        let stat = stat_name.to_lowercase();
        if let Some(v) = self.limit_float(&format!("{}_{}", stat, severity)) {
            return Some(v);
        }
        self.limit_float(&format!("{}_{}", self.plugin_name, severity))
    }

    /// Log tag for a stat (`<stat>_log`, else `<plugin>_log`, else the
    /// caller's default — upstream `get_limit_log` parity).
    pub fn get_limit_log(&self, stat_name: &str, default: bool) -> bool {
        let flag = |key: String| match self.limits.get(&key) {
            Some(LimitValue::List(l)) => l.first().map(|s| s.to_lowercase() == "true"),
            Some(LimitValue::Float(f)) => Some(*f != 0.0),
            None => None,
        };
        let stat = stat_name.to_lowercase();
        flag(format!("{}_log", stat))
            .or_else(|| flag(format!("{}_log", self.plugin_name)))
            .unwrap_or(default)
    }

    /// Alert severity for a measurement (upstream `get_alert` parity):
    /// percent-normalizes `current` against `maximum`, walks
    /// careful→warning→critical, honors `minimum` (below → CAREFUL),
    /// returns DEFAULT when unset/zero, appends `_LOG` + records an
    /// event when the log tag is set, and tracks the trigger.
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
        if !highlight_zero && current == 0.0 {
            return "DEFAULT".into();
        }
        if maximum == 0.0 {
            return "DEFAULT".into();
        }
        let value = current * 100.0 / maximum;
        if !value.is_finite() {
            return "DEFAULT".into();
        }
        // stat_name: plugin[_action_key][_header], lowercased
        // (upstream `get_stat_name` parity).
        let mut stat_name = self.plugin_name.to_string();
        if let Some(ak) = action_key {
            if !ak.is_empty() {
                stat_name.push('_');
                stat_name.push_str(ak);
            }
        }
        if !header.is_empty() {
            stat_name.push('_');
            stat_name.push_str(header);
        }
        let stat_name = stat_name.to_lowercase();

        // NOTE: mirrors upstream's chain, whose `ret = 'MAX' if is_max`
        // initializer never survives (every path below overwrites it),
        // so the initializer is dropped and only the chain remains.
        // `is_max` is kept in the signature for call-site parity.
        let _ = is_max;
        let mut ret;
        let critical = self.get_limit("critical", &stat_name);
        let warning = self.get_limit("warning", &stat_name);
        let careful = self.get_limit("careful", &stat_name);
        if critical.is_some_and(|c| value >= c) {
            ret = "CRITICAL";
        } else if warning.is_some_and(|w| value >= w) {
            ret = "WARNING";
        } else if careful.is_some_and(|c| value >= c) {
            ret = "CAREFUL";
        } else if critical.is_none() && warning.is_none() && careful.is_none() {
            ret = "DEFAULT";
        } else {
            ret = "OK";
        }
        if current < minimum {
            ret = "CAREFUL";
        }

        let mut out = ret.to_string();
        if self.get_limit_log(&stat_name, log.unwrap_or(false)) && ret != "DEFAULT" {
            out.push_str("_LOG");
            if let Some(ev) = events {
                if let Some(sev) = severity_of(ret) {
                    ev.push(Event {
                        severity: sev,
                        stat: stat_name.clone(),
                        value,
                        timestamp: std::time::SystemTime::now(),
                    });
                }
            }
        }
        self.thresholds.insert(stat_name, ret.to_string());
        out
    }

    /// `get_alert` with logging enabled (upstream `get_alert_log`).
    pub fn get_alert_log(
        &mut self,
        current: f64,
        maximum: f64,
        header: &str,
        events: Option<&mut EventLog>,
    ) -> String {
        self.get_alert(current, 0.0, maximum, header, None, false, false, Some(true), events)
    }

    pub fn apply_default_limits(&mut self, entries: &[(&str, f64, f64, f64)], ncpu: u64) {
        for (header, careful, warning, critical) in entries {
            // Stored plugin-prefixed like `load_limits` does
            // (`[load] careful` → `load_careful`).
            let prefix = if header.is_empty() {
                format!("{}_", self.plugin_name)
            } else {
                format!("{}_{}_", self.plugin_name, header)
            };
            let mut set = |kind: &str, v: f64| {
                let key = format!("{}{}", prefix, kind);
                if !self.limits.contains_key(&key) {
                    self.limits.insert(key, LimitValue::Float(v));
                }
            };
            if *header == "ctx_switches" && self.plugin_name == "cpu" {
                let base = 500_000.0 * 0.10 * ncpu.max(1) as f64;
                set("careful", base * 0.80);
                set("warning", base * 0.90);
                set("critical", base);
            } else {
                set("careful", *careful);
                set("warning", *warning);
                set("critical", *critical);
            }
        }
    }}


/// Built-in careful/warning/critical defaults per plugin
/// (upstream `config.py` `set_default_cwc` parity). Each entry is
/// `(header_or_empty, careful, warning, critical)`; the cpu
/// `ctx_switches` row is scaled by logical CPU count in
/// `apply_default_limits`.
pub fn default_limit_entries(plugin: &str) -> Vec<(&'static str, f64, f64, f64)> {
    match plugin {
        "quicklook" => vec![("cpu", 50.0, 70.0, 90.0), ("mem", 50.0, 70.0, 90.0), ("swap", 50.0, 70.0, 90.0)],
        "cpu" => vec![
            ("user", 50.0, 70.0, 90.0),
            ("system", 50.0, 70.0, 90.0),
            ("steal", 50.0, 70.0, 90.0),
            ("iowait", 50.0, 70.0, 90.0),
            ("ctx_switches", f64::NAN, f64::NAN, f64::NAN),
        ],
        "percpu" => vec![("user", 50.0, 70.0, 90.0), ("system", 50.0, 70.0, 90.0)],
        "load" => vec![("", 0.7, 1.0, 5.0)],
        "mem" | "memswap" | "fs" => vec![("", 50.0, 70.0, 90.0)],
        "network" => vec![("rx", 50.0, 70.0, 90.0), ("tx", 50.0, 70.0, 90.0)],
        "sensors" => vec![
            ("temperature_hdd", 45.0, 52.0, 60.0),
            ("battery", 70.0, 80.0, 90.0),
        ],
        "processlist" => vec![("cpu", 50.0, 70.0, 90.0), ("mem", 50.0, 70.0, 90.0)],
        _ => Vec::new(),
    }
}

/// Severity for an alert trigger word (`None` for DEFAULT/OK/MAX).
fn severity_of(trigger: &str) -> Option<Severity> {
    match trigger {
        "CAREFUL" => Some(Severity::Careful),
        "WARNING" => Some(Severity::Warning),
        "CRITICAL" => Some(Severity::Critical),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LimitValue {
    Float(f64),
    List(Vec<String>),
}
