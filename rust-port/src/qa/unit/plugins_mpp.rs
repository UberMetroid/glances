//! Tests for the MPP plugin — Rockchip encoder/decoder load formatting.

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins::mpp::{mpp_to_value, read_load_monitor, MppChannel, NAME};

#[test]
fn name_and_register() {
    let s = GlancesStats::new(1.0);
    crate::plugins::mpp::register(&s);
    assert!(s.plugin_names().contains(&NAME));
}

#[test]
fn get_key_returns_channel_id() {
    let p = crate::plugins::mpp::MppPlugin::new();
    assert_eq!(p.get_key(), Some("channel_id"));
}

#[test]
fn read_load_monitor_parses_rockchip_bsp_format() {
    // The combined load_monitor file uses "Encoder: N Decoder: M".
    assert_eq!(read_load_monitor_helper("Encoder: 12 Decoder: 5"), Some((12.0, 5.0)));
    assert_eq!(read_load_monitor_helper("Encoder: 0 Decoder: 0"), Some((0.0, 0.0)));
    assert_eq!(read_load_monitor_helper("garbage no values"), None);
    // Only one value present → can't pair.
    assert_eq!(read_load_monitor_helper("Encoder: 42"), None);
}

fn read_load_monitor_helper(s: &str) -> Option<(f64, f64)> {
    let dir = tempdir();
    let path = dir.join("load_monitor");
    std::fs::write(&path, s).unwrap();
    read_load_monitor(&path)
}

#[test]
fn mpp_to_value_emits_canonical_keys() {
    let c = MppChannel {
        channel_id: "dri0".into(),
        encoder_load_pct: Some(25.0),
        decoder_load_pct: Some(10.0),
    };
    let v = mpp_to_value(&c);
    let obj = v.as_object().expect("object");
    assert_eq!(obj.get("channel_id").and_then(Value::as_str), Some("dri0"));
    assert_eq!(obj.get("encoder_load_pct").and_then(Value::as_f64), Some(25.0));
    assert_eq!(obj.get("decoder_load_pct").and_then(Value::as_f64), Some(10.0));
}

#[test]
fn mpp_to_value_handles_null_loads() {
    let c = MppChannel {
        channel_id: "dri1".into(),
        encoder_load_pct: None,
        decoder_load_pct: Some(80.0),
    };
    let v = mpp_to_value(&c);
    let obj = v.as_object().unwrap();
    assert!(matches!(obj.get("encoder_load_pct"), Some(Value::Null)));
    assert_eq!(obj.get("decoder_load_pct").and_then(Value::as_f64), Some(80.0));
}

#[test]
fn plugin_update_emits_empty_array_without_debugfs() {
    // /sys/kernel/debug is typically not readable as a non-root user.
    // The plugin must not error — it must emit [].
    let mut p = crate::plugins::mpp::MppPlugin::new();
    p.update().expect("update must not error");
    assert!(p.stats().as_array().is_some(), "stats always array");
}

#[test]
fn plugin_reset_clears_stats() {
    let mut p = crate::plugins::mpp::MppPlugin::new();
    p.update().expect("update ok");
    p.reset();
    assert!(p.stats().as_array().unwrap().is_empty());
}

// Tiny test-only tempdir helper. std::env::temp_dir() is unique per
// process when we suffix with pid + nanos; good enough for unit tests.
fn tempdir() -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("glances-rs-mpp-{}-{}", pid, nanos));
    std::fs::create_dir_all(&dir).expect("create tempdir");
    dir
}
