//! Tests for the cloud plugin — provider parsing, probe failure paths,
//! and registry wiring.

use std::net::{SocketAddr, SocketAddrV4, TcpListener};
use std::thread;
use std::time::Duration;

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins::cloud::{
    self, http_get, none_object, parse_aws, parse_azure, parse_gcp, probe, CloudPlugin, NAME,
};

const AWS_BODY: &str = r#"{
  "instanceId": "i-0123456789abcdef0",
  "region": "us-east-1",
  "availabilityZone": "us-east-1a"
}"#;

const GCP_BODY: &str = r#"{
  "instance": {
    "id": 1234567890123456789,
    "zone": "projects/my-proj/zones/us-central1-a"
  }
}"#;

const AZURE_BODY: &str = r#"{
  "compute": {
    "vmId": "4c1f0a3e-1b9d-4f3a-9c1f-0a3e1b9d4f3a",
    "location": "eastus",
    "availabilityZone": "1"
  }
}"#;

#[test]
fn parse_aws_extracts_instance_region_and_zone() {
    let v = parse_aws(AWS_BODY).expect("aws parse succeeds");
    let m = v.as_object().unwrap();
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("aws"));
    assert_eq!(m.get("instance_id").and_then(Value::as_str), Some("i-0123456789abcdef0"));
    assert_eq!(m.get("region").and_then(Value::as_str), Some("us-east-1"));
    assert_eq!(m.get("zone").and_then(Value::as_str), Some("us-east-1a"));
}

#[test]
fn parse_gcp_splits_zone_path_into_region_and_letter() {
    let v = parse_gcp(GCP_BODY).expect("gcp parse succeeds");
    let m = v.as_object().unwrap();
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("gcp"));
    assert_eq!(m.get("region").and_then(Value::as_str), Some("us-central1"));
    assert_eq!(m.get("zone").and_then(Value::as_str), Some("a"));
    // id was a u64-like integer; it must serialise back to a string.
    let id = m.get("instance_id").and_then(Value::as_str).expect("id is string");
    assert_eq!(id, "1234567890123456789");
}

#[test]
fn parse_azure_extracts_compute_fields() {
    let v = parse_azure(AZURE_BODY).expect("azure parse succeeds");
    let m = v.as_object().unwrap();
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("azure"));
    assert_eq!(m.get("instance_id").and_then(Value::as_str), Some("4c1f0a3e-1b9d-4f3a-9c1f-0a3e1b9d4f3a"));
    assert_eq!(m.get("region").and_then(Value::as_str), Some("eastus"));
    assert_eq!(m.get("zone").and_then(Value::as_str), Some("1"));
}

#[test]
fn probe_unreachable_address_returns_none_object() {
    // 127.0.0.1:1 — port 1 is normally closed on Linux. The connect is
    // either refused immediately or times out; either way the probe
    // path must yield the `{provider: "none"}` sentinel.
    let addr: SocketAddr = SocketAddr::V4(SocketAddrV4::new(
        std::net::Ipv4Addr::new(127, 0, 0, 1),
        1,
    ));
    // Bound cap is much smaller than TOTAL_BUDGET so the test stays
    // fast even if the OS retries the connect several times.
    let v = probe(addr);
    let m = v.as_object().expect("probe always returns an object");
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("none"));
}

#[test]
fn probe_returns_none_when_listener_accepts_but_never_responds() {
    // Bind a listener that accepts the TCP connection but never writes
    // a response. With our small read timeout, the probe must time out
    // and fall through to the GCP / Azure probes, then return none.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    // Background thread accepts (so the connect succeeds) but never writes.
    thread::spawn(move || {
        for stream in listener.incoming() {
            // Drop the stream — the client gets EOF eventually.
            let _ = stream;
        }
    });
    let v = probe(addr);
    let m = v.as_object().expect("probe always returns an object");
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("none"));
}

#[test]
fn http_get_returns_none_on_unreachable_address() {
    let addr: SocketAddr = SocketAddr::V4(SocketAddrV4::new(
        std::net::Ipv4Addr::new(127, 0, 0, 1),
        1,
    ));
    let r = http_get(addr, "/anything", &[], Duration::from_millis(100));
    assert!(r.is_none());
}

#[test]
fn none_object_has_provider_none_key() {
    let v = none_object();
    let m = v.as_object().expect("sentinel is an object");
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("none"));
    assert_eq!(m.len(), 1, "sentinel has no other keys");
}

#[test]
fn plugin_register_and_initial_stats() {
    let s = GlancesStats::new(1.0);
    cloud::register(&s);
    assert!(s.plugin_names().contains(&NAME));
    let p = CloudPlugin::new();
    // Initial stats shape — `{provider: "none"}` sentinel.
    let m = p.stats().as_object().expect("stats is object");
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("none"));
    // get_key returns the discriminating field.
    assert_eq!(p.get_key(), Some("provider"));
}

#[test]
fn plugin_update_caches_probe_result() {
    // Regression: the metadata probe (3x400ms timeouts off-cloud)
    // must run once, not every tick. Both updates return the same
    // object; only the first touches the network.
    let mut p = CloudPlugin::new();
    p.update().expect("first update ok");
    let first = p.stats().clone();
    p.update().expect("second update ok");
    assert_eq!(&first, p.stats());
    assert!(p.stats().as_object().unwrap().contains_key("provider"));
}

// ---- fake metadata backends (full probe sequence over real TCP) ----

/// Serve `routes` (path -> (status, body)) until 4 connections or 10s.
/// Unlisted paths get 404. Returns the loopback address to probe.
fn serve_metadata(routes: std::collections::HashMap<String, (u16, String)>) -> SocketAddr {
    use std::io::{Read, Write};
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    listener.set_nonblocking(true).expect("nonblocking");
    thread::spawn(move || {
        let end = std::time::Instant::now() + Duration::from_secs(10);
        let mut handled = 0;
        loop {
            match listener.accept() {
                Ok((mut s, _)) => {
                    let mut req = vec![0u8; 8192];
                    let mut got = 0;
                    s.set_read_timeout(Some(Duration::from_secs(2))).ok();
                    while got < req.len() {
                        match s.read(&mut req[got..]) {
                            Ok(0) => break,
                            Ok(n) => {
                                got += n;
                                if req[..got].windows(4).any(|w| w == b"\r\n\r\n") {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    let path = String::from_utf8_lossy(&req[..got])
                        .lines().next().unwrap_or("")
                        .split_whitespace().nth(1).unwrap_or("").to_string();
                    let (st, body) = routes.get(&path).cloned().unwrap_or((404, String::new()));
                    let phrase = if st == 200 { "OK" } else { "Not Found" };
                    let _ = s.write_all(
                        format!("HTTP/1.1 {st} {phrase}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()).as_bytes());
                    handled += 1;
                    if handled >= 4 {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() > end {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    addr
}

fn routes_for(aws: bool, gcp: bool, azure: bool) -> std::collections::HashMap<String, (u16, String)> {
    let mut m = std::collections::HashMap::new();
    if aws {
        m.insert("/latest/dynamic/instance-identity/document".into(), (200, AWS_BODY.into()));
    }
    if gcp {
        m.insert("/computeMetadata/v1/instance/?recursive=true".into(), (200, GCP_BODY.into()));
    }
    if azure {
        m.insert("/metadata/instance?api-version=2021-02-01&format=json".into(), (200, AZURE_BODY.into()));
    }
    m
}

#[test]
fn probe_aws_answer_wins_over_tcp() {
    let m = probe(serve_metadata(routes_for(true, true, true)))
        .as_object().expect("object").clone();
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("aws"));
    assert_eq!(m.get("instance_id").and_then(Value::as_str), Some("i-0123456789abcdef0"));
    assert_eq!(m.get("region").and_then(Value::as_str), Some("us-east-1"));
}

#[test]
fn probe_falls_through_to_gcp() {
    let m = probe(serve_metadata(routes_for(false, true, true)))
        .as_object().expect("object").clone();
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("gcp"));
    assert_eq!(m.get("region").and_then(Value::as_str), Some("us-central1"));
    assert_eq!(m.get("zone").and_then(Value::as_str), Some("a"));
}

#[test]
fn probe_falls_through_to_azure() {
    let m = probe(serve_metadata(routes_for(false, false, true)))
        .as_object().expect("object").clone();
    assert_eq!(m.get("provider").and_then(Value::as_str), Some("azure"));
    assert_eq!(m.get("instance_id").and_then(Value::as_str),
        Some("4c1f0a3e-1b9d-4f3a-9c1f-0a3e1b9d4f3a"));
}
