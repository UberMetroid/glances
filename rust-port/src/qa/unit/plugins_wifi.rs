//! Tests for the Wi-Fi plugin — /proc/net/wireless parser.

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins::wifi::{parse, parse_line, wifi_to_value, WifiLink, NAME};

const FIXTURE: &str = "\
Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE
 face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22
wlan0: 0000   70.  -40.  -95.   0    0    0     0     0        0
 wlan1: 0001   55.  -65.  -90.   0    0    0     0     0        0
";

#[test]
fn name_and_register() {
    let s = GlancesStats::new(1.0);
    crate::plugins::wifi::register(&s);
    assert!(s.plugin_names().contains(&NAME));
}

#[test]
fn get_key_returns_interface() {
    let p = crate::plugins::wifi::WifiPlugin::new();
    assert_eq!(p.get_key(), Some("interface"));
}

#[test]
fn parse_line_extracts_link_level_and_noise() {
    let link = parse_line("wlan0: 0000   70.  -40.  -95.   0    0    0     0     0        0")
        .expect("must parse");
    assert_eq!(link.interface, "wlan0");
    assert_eq!(link.link_quality_pct, Some(70.0));
    assert_eq!(link.signal_dbm, Some(-40.0));
    assert_eq!(link.ssid, "?");
}

#[test]
fn parse_line_returns_none_for_header_rows() {
    // Header rows have non-hex tokens in the first column → rejected.
    assert!(parse_line("Inter-| sta-|   Quality        |   Discarded packets").is_none());
    assert!(parse_line(" face | tus | link level noise |  nwid  crypt   frag  retry   misc").is_none());
}

#[test]
fn parse_line_returns_none_for_malformed_row() {
    // No dotted tokens at all → not a /proc/net/wireless row.
    assert!(parse_line("garbage: no data here").is_none());
}

#[test]
fn parse_full_fixture_yields_two_links() {
    let links = parse(FIXTURE);
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].interface, "wlan0");
    assert_eq!(links[0].link_quality_pct, Some(70.0));
    assert_eq!(links[1].interface, "wlan1");
    assert_eq!(links[1].link_quality_pct, Some(55.0));
    assert_eq!(links[1].signal_dbm, Some(-65.0));
}

#[test]
fn wifi_to_value_emits_canonical_keys_with_nulls_for_unknown_bitrate() {
    let link = WifiLink {
        interface: "wlan0".into(),
        ssid: "?".into(),
        signal_dbm: Some(-50.0),
        bitrate_mbps: None, // not available from /proc/net/wireless
        link_quality_pct: Some(80.0),
    };
    let v = wifi_to_value(&link);
    let obj = v.as_object().expect("object");
    assert_eq!(obj.get("interface").and_then(Value::as_str), Some("wlan0"));
    assert_eq!(obj.get("ssid").and_then(Value::as_str), Some("?"));
    assert_eq!(obj.get("signal_dbm").and_then(Value::as_f64), Some(-50.0));
    assert_eq!(obj.get("link_quality_pct").and_then(Value::as_f64), Some(80.0));
    assert!(matches!(obj.get("bitrate_mbps"), Some(Value::Null)));
}

#[test]
fn plugin_update_emits_empty_array_when_wireless_unavailable() {
    // Most test hosts don't have /proc/net/wireless — must not error.
    let mut p = crate::plugins::wifi::WifiPlugin::new();
    p.update().expect("update must not error");
    assert!(p.stats().as_array().is_some());
}
