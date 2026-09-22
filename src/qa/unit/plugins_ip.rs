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
    // localhost:1 — nothing listens there; ECONNREFUSED is immediate,
    // so the fetch must fail quickly rather than hang or panic.
    let cfg = ip::PublicCfg {
        host: "127.0.0.1".into(),
        port: 1,
        path: "/".into(),
        fields: Vec::new(),
        basic_auth: None,
        refresh_secs: 300,
    };
    let result = ip::fetch_public_ip(&cfg);
    assert!(result.is_err(), "fetch from reserved addr should fail");
}

#[test]
fn extract_ip_validates_body() {
    // Bare literal is accepted; HTML/garbage is not.
    assert_eq!(ip::extract_ip("1.2.3.4", &[]).unwrap(), "1.2.3.4");
    assert_eq!(ip::extract_ip("::1", &[]).unwrap(), "::1");
    assert!(ip::extract_ip("<html>oops</html>", &[]).is_err());
    // With fields, the value must come from a configured JSON key.
    let fields = vec!["ip".to_string(), "query".to_string()];
    assert_eq!(ip::extract_ip(r#"{"query":"5.6.7.8"}"#, &fields).unwrap(), "5.6.7.8");
    assert!(ip::extract_ip(r#"{"other":"5.6.7.8"}"#, &fields).is_err());
    assert!(ip::extract_ip(r#"{"ip":"<b>x</b>"}"#, &fields).is_err());
}

#[test]
fn fib_trie_pairs_ip_with_following_annotation() {
    // Real /proc/net/fib_trie shape: the `|-- <ip>` node line precedes
    // its `/32 host LOCAL` annotation — the parser must pair each
    // marker with the IP ABOVE it, not the one below.
    let text = "\
Main:
  +-- 0.0.0.0/0 3 0 5
     |-- 10.0.0.0
        /24 universe UNICAST
     +-- 10.0.0.2
        /32 host LOCAL
     |-- 10.255.255.255
        /32 link BROADCAST
Local:
  +-- 0.0.0.0/0 3 0 5
     |-- 10.0.0.2
        /32 host LOCAL
     |-- 127.0.0.0
        /8 host LOCAL
     |-- 127.0.0.1
        /32 host LOCAL
";
    let ips = ip::parse_fib_trie(text);
    // 10.0.0.0 is UNICAST (not host LOCAL) — excluded; 10.255.255.255
    // is BROADCAST — excluded; 10.0.0.2 is LOCAL in both tables (may
    // appear once or twice); loopbacks are collected but callers skip.
    assert!(ips.iter().all(|i| i == "10.0.0.2" || i.starts_with("127.")),
        "unexpected IPs: {:?}", ips);
    assert!(ips.contains(&"10.0.0.2".to_string()));
    assert!(!ips.contains(&"10.0.0.0".to_string()));
    assert!(!ips.contains(&"10.255.255.255".to_string()));
}

#[test]
fn address_for_iface_picks_ip_in_subnet() {
    // Multi-homed: eth0 holds 192.168.3.0/24, eth1 holds 10.0.0.0/24.
    // The reported address must be eth0's IP when eth0 owns the route.
    let routes = vec![
        ip::RouteRow { iface: "eth0".into(), dest: ip::ipv4_to_u32("192.168.3.0").unwrap(),
            gateway: 0, mask: ip::ipv4_to_u32("255.255.255.0").unwrap() },
        ip::RouteRow { iface: "eth1".into(), dest: ip::ipv4_to_u32("10.0.0.0").unwrap(),
            gateway: 0, mask: ip::ipv4_to_u32("255.255.255.0").unwrap() },
    ];
    let locals = vec!["10.0.0.7".to_string(), "192.168.3.50".to_string(), "127.0.0.1".to_string()];
    assert_eq!(ip::address_for_iface("eth0", &routes, &locals), "192.168.3.50");
    assert_eq!(ip::address_for_iface("eth1", &routes, &locals), "10.0.0.7");
    // Unknown iface falls back to first non-loopback local IP.
    assert_eq!(ip::address_for_iface("eth9", &routes, &locals), "10.0.0.7");
}

#[test]
fn attribute_ips_routes_lo_and_eliminates_tunnel() {
    // Real-world shape: only eth0 has a main-table subnet route;
    // loopback and the tunnel leave no route rows at all.
    let routes = vec![
        ip::RouteRow { iface: "eth0".into(), dest: ip::ipv4_to_u32("192.168.3.0").unwrap(),
            gateway: 0, mask: ip::ipv4_to_u32("255.255.255.0").unwrap() },
    ];
    let locals = vec!["192.168.3.50".to_string(), "127.0.0.1".to_string(), "100.117.155.12".to_string()];
    let up = vec!["lo".to_string(), "eth0".to_string(), "tailscale0".to_string()];
    let m = ip::attribute_ips(&up, &routes, &locals);
    assert_eq!(m.get("eth0").cloned().unwrap_or_default(), vec!["192.168.3.50".to_string()]);
    assert_eq!(m.get("lo").cloned().unwrap_or_default(), vec!["127.0.0.1".to_string()]);
    assert_eq!(m.get("tailscale0").cloned().unwrap_or_default(), vec!["100.117.155.12".to_string()]);
    // A second bare interface makes it ambiguous — silence, not a guess.
    let up2 = vec!["lo".to_string(), "eth0".to_string(), "tailscale0".to_string(), "eth1".to_string()];
    let m2 = ip::attribute_ips(&up2, &routes, &locals);
    assert!(!m2.contains_key("tailscale0"));
    assert!(!m2.contains_key("eth1"));
}

#[test]
fn resolve_api_url_env_wins_blanks_off() {
    assert_eq!(ip::resolve_api_url(Some("http://cfg/"), Some("http://env/")).as_deref(), Some("http://env/"));
    assert_eq!(ip::resolve_api_url(Some("http://cfg/"), None).as_deref(), Some("http://cfg/"));
    assert_eq!(ip::resolve_api_url(None, Some("http://env/")).as_deref(), Some("http://env/"));
    assert_eq!(ip::resolve_api_url(Some("  "), Some("")), None);
    assert_eq!(ip::resolve_api_url(None, None), None);
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