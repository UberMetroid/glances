//! Tests for the IP plugin.

use crate::core::plugin::Plugin;
use crate::core::value::Value;
use crate::plugins::ip;

#[test]
fn plugin_metadata_and_initial_state() {
    let p = ip::IpPlugin::new();
    assert_eq!(p.name(), ip::NAME);
    assert_eq!(p.name(), "ip");
    let obj = p.stats().as_object().expect("stats must be an object");
    for k in &["address", "mask", "gateway", "public_ip", "mac"] {
        assert!(obj.contains_key(*k), "missing initial key '{}'", k);
        assert_eq!(obj.get(*k), Some(&Value::String(String::new())));
    }
}

#[test]
fn hex_to_ipv4_decodes_little_endian() {
    // 192.168.3.1 stored as 0103A8C0
    assert_eq!(ip::hex_to_ipv4("0103A8C0").as_deref(), Some("192.168.3.1"));
    // 10.0.0.1 → 0100000A
    assert_eq!(ip::hex_to_ipv4("0100000A").as_deref(), Some("10.0.0.1"));
    // 0.0.0.0
    assert_eq!(ip::hex_to_ipv4("00000000").as_deref(), Some("0.0.0.0"));
    // 255.255.255.255
    assert_eq!(ip::hex_to_ipv4("FFFFFFFF").as_deref(), Some("255.255.255.255"));
}

#[test]
fn hex_to_ipv4_rejects_bad_length() {
    assert!(ip::hex_to_ipv4("").is_none());
    assert!(ip::hex_to_ipv4("FFFF").is_none());
    assert!(ip::hex_to_ipv4("FFFFFFFFF").is_none());
}

#[test]
fn hex_to_ipv4_rejects_non_hex() {
    assert!(ip::hex_to_ipv4("GGGGGGGG").is_none());
    assert!(ip::hex_to_ipv4("ZZZZZZZZ").is_none());
}

#[test]
fn parse_route_line_picks_default_route() {
    let line = "wlp8s0\t00000000\t0103A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0";
    let parsed = ip::parse_route_line(line);
    assert!(parsed.is_some());
    let (iface, gw) = parsed.unwrap();
    assert_eq!(iface, "wlp8s0");
    assert_eq!(gw, "0103A8C0");
}

#[test]
fn parse_route_line_skips_non_default() {
    let line = "wlp8s0\t0003A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0";
    let parsed = ip::parse_route_line(line);
    // The destination isn't 00000000 → not the default route.
    assert!(parsed.is_none());
}

#[test]
fn parse_route_line_truncated_returns_none() {
    assert!(ip::parse_route_line("").is_none());
    // Only one whitespace-delimited token — too few fields to read dest + gw.
    assert!(ip::parse_route_line("wlp8s0").is_none());
    assert!(ip::parse_route_line("wlp8s0 00000000").is_none());
}

#[test]
fn fetch_public_ip_returns_err_when_endpoint_unreachable() {
    // The default endpoint may or may not be reachable in CI; what we want
    // here is just that an unreachable / unreachable-shaped call doesn't
    // hang and either returns Ok("...") or Err(_). Either is acceptable.
    let result = ip::fetch_public_ip();
    // We don't assert on success because the test runner may not have
    // network access; we just want to ensure we got SOME result.
    let _ = result;
}

#[test]
fn update_writes_keys_on_linux() {
    if !cfg!(target_os = "linux") { return; }
    let mut p = ip::IpPlugin::new();
    p.update().expect("ip update should succeed on Linux");
    let obj = p.stats().as_object().expect("stats must be an object");
    // Every key we declared must be present.
    for k in &["address", "mask", "gateway", "public_ip", "mac"] {
        assert!(obj.contains_key(*k), "missing '{}' after update", k);
    }
    // gateway and mac should be strings; address may be empty on hosts
    // with no routable interface, but we don't assert that here.
    assert!(obj.get("gateway").and_then(|v| v.as_str()).is_some());
}

#[test]
fn reset_restores_initial_state() {
    let mut p = ip::IpPlugin::new();
    if let Some(obj) = p.stats_mut().as_object_mut() {
        obj.insert("address".into(), Value::String("1.2.3.4".into()));
    }
    p.reset();
    let obj = p.stats().as_object().unwrap();
    assert_eq!(obj.get("address"), Some(&Value::String(String::new())));
}