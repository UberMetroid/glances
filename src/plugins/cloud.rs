//! Cloud plugin — which cloud provider (if any) hosts this machine.
//!
//! Each major provider exposes an instance-metadata service at the
//! link-local address 169.254.169.254:80. The three recognised
//! providers are probed in sequence; the first to return a 2xx
//! response wins.
//!
//! Output: in cloud → `{ provider, instance_id, region, zone }`;
//! not in cloud → `{ provider: "none" }`. `update()` never
//! propagates errors — a non-cloud host just reports "none".

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::time::{Duration, Instant};

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::plugins::json;

pub const NAME: &str = "cloud";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(CloudPlugin::new()));
}

pub const METADATA_IP: Ipv4Addr = Ipv4Addr::new(169, 254, 169, 254);
pub const METADATA_PORT: u16 = 80;
pub const METADATA_ADDR: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(METADATA_IP, METADATA_PORT));

/// Wall-clock budget for the whole probe sequence.
pub const TOTAL_BUDGET: Duration = Duration::from_millis(1000);

/// Per-provider ceiling — leaves headroom for the other two probes
/// even if one stalls.
pub const PER_PROBE_TIMEOUT: Duration = Duration::from_millis(400);

/// Send one HTTP/1.1 GET, return the body on a 2xx status.
/// `timeout` bounds both the connect and the response read. Any
/// failure returns None.
pub fn http_get(
    addr: SocketAddr,
    path: &str,
    headers: &[&str],
    timeout: Duration,
) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: metadata\r\nConnection: close\r\n");
    for h in headers {
        req.push_str(h);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    let sep = raw.find("\r\n\r\n")?;
    let head = &raw[..sep];
    let body = raw[sep + 4..].to_string();
    if !head.starts_with("HTTP/1.") {
        return None;
    }
    let space = head[8..].find(' ')?;
    let code: u32 = head[8..8 + space].parse().ok()?;
    (200..300).contains(&code).then_some(body)
}

/// AWS instance-identity document.
pub fn parse_aws(body: &str) -> Option<Value> {
    let v = json::parse_object(body)?;
    let get = |k: &str| -> String {
        v.as_object()
            .and_then(|o| o.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Some(provider_value(
        "aws",
        &get("instanceId"),
        &get("region"),
        &get("availabilityZone"),
    ))
}

/// GCP metadata-recursive body. `instance.zone` is a qualified path
/// like `projects/.../zones/us-central1-a`; the tail splits into
/// region + zone on the last `-`.
pub fn parse_gcp(body: &str) -> Option<Value> {
    let v = json::parse_object(body)?;
    let instance = v.as_object()?.get("instance").and_then(Value::as_object);
    let id = instance
        .and_then(|m| m.get("id"))
        .map(value_to_string)
        .unwrap_or_default();
    let zone_full = instance
        .and_then(|m| m.get("zone"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // Tail after the final "/" looks like "us-central1-a".
    let (region, zone) = match zone_full.rsplit_once('/') {
        Some((_, tail)) => match tail.rsplit_once('-') {
            Some((r, z)) => (r.to_string(), z.to_string()),
            None => (String::new(), tail.to_string()),
        },
        None => (String::new(), zone_full),
    };
    Some(provider_value("gcp", &id, &region, &zone))
}

/// Azure metadata body (`{ compute: { vmId, location, ... } }`).
pub fn parse_azure(body: &str) -> Option<Value> {
    let v = json::parse_object(body)?;
    let compute = v.as_object()?.get("compute").and_then(Value::as_object);
    let get = |k: &str| {
        compute
            .and_then(|m| m.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Some(provider_value(
        "azure",
        &get("vmId"),
        &get("location"),
        &get("availabilityZone"),
    ))
}

/// Build the `{provider, instance_id, region, zone}` object.
fn provider_value(provider: &str, id: &str, region: &str, zone: &str) -> Value {
    Value::Object(BTreeMap::from([
        ("provider".into(), Value::String(provider.into())),
        ("instance_id".into(), Value::String(id.into())),
        ("region".into(), Value::String(region.into())),
        ("zone".into(), Value::String(zone.into())),
    ]))
}

/// `{provider: "none"}` sentinel.
pub fn none_object() -> Value {
    let mut m = BTreeMap::new();
    m.insert("provider".into(), Value::String("none".into()));
    Value::Object(m)
}

/// Flatten a JSON scalar to text (for numeric `id` fields like GCP's).
fn value_to_string(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// One provider probe: `(path, headers) → parser`. The HTTP fetch is
/// shared so the connect/read plumbing is written once.
type Probe = (&'static str, &'static [&'static str], fn(&str) -> Option<Value>);
const PROBES: &[Probe] = &[
    ("/latest/dynamic/instance-identity/document", &[], parse_aws),
    (
        "/computeMetadata/v1/instance/?recursive=true",
        &["Metadata-Flavor: Google"],
        parse_gcp,
    ),
    (
        "/metadata/instance?api-version=2021-02-01&format=json",
        &["Metadata: true"],
        parse_azure,
    ),
];

/// Probe the metadata service. Returns the parsed provider object or
/// `none_object()` on any failure. Total wall-clock time is bounded
/// by `TOTAL_BUDGET`.
pub fn probe(addr: SocketAddr) -> Value {
    let deadline = Instant::now() + TOTAL_BUDGET;
    for (path, headers, parser) in PROBES {
        if Instant::now() >= deadline {
            break;
        }
        if let Some(body) = http_get(addr, path, headers, PER_PROBE_TIMEOUT)
            && let Some(v) = parser(&body)
        {
            return v;
        }
    }
    none_object()
}

pub struct CloudPlugin {
    base: GlancesPluginModel,
    /// First-tick result, cached — cloud identity is boot-stable,
    /// and re-probing costs ~1.2s per tick on non-cloud hosts (three
    /// 400ms connect timeouts).
    cached: Option<Value>,
}

impl CloudPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, none_object()),
            cached: None,
        }
    }
}

impl Default for CloudPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for CloudPlugin {
    fn name(&self) -> &'static str {
        NAME
    }
    fn reset(&mut self) {
        self.base.reset();
    }
    fn stats(&self) -> &Value {
        &self.base.stats
    }
    fn model(&self) -> Option<&GlancesPluginModel> {
        Some(&self.base)
    }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> {
        Some(&mut self.base)
    }
    fn stats_mut(&mut self) -> &mut Value {
        &mut self.base.stats
    }
    fn get_key(&self) -> Option<&'static str> {
        Some("provider")
    }
    fn update(&mut self) -> Result<()> {
        if self.cached.is_none() {
            self.cached = Some(probe(METADATA_ADDR));
        }
        if let Some(v) = &self.cached {
            self.base.stats = v.clone();
        }
        Ok(())
    }
}
