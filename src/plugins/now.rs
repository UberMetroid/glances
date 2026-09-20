//! Now plugin — current local date/time + ISO 8601.

use std::collections::BTreeMap;
use std::time::SystemTime;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "now";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(NowPlugin::new()));
}

pub struct NowPlugin { base: GlancesPluginModel }

impl NowPlugin {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        m.insert("iso".into(), Value::String(String::new()));
        m.insert("utc".into(), Value::Float(0.0));
        Self { base: GlancesPluginModel::new(NAME, Value::Object(m)) }
    }
}

impl Plugin for NowPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn update(&mut self) -> Result<()> {
        let now = SystemTime::now();
        let dur = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
        let secs = dur.as_secs_f64();
        if let Some(obj) = self.base.stats.as_object_mut() {
            obj.insert("iso".into(), Value::String(iso_utc(dur.as_secs())));
            obj.insert("utc".into(), Value::Float(secs));
        }
        Ok(())
    }
}

/// Days->(year, month, day) — Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `YYYY-MM-DDTHH:MM:SSZ` for a unix timestamp (UTC).
pub fn iso_utc(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86400) as i64;
    let rem = epoch_secs % 86400;
    let (y, mo, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, mo, d, rem / 3600, (rem / 60) % 60, rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(951782400), "2000-02-29T00:00:00Z"); // leap day
        assert_eq!(iso_utc(1700000000), "2023-11-14T22:13:20Z");
    }
}
