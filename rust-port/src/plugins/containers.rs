//! Containers plugin — list of running containers (Docker engine).
//!
//! Mirrors `glances/plugins/containers/__init__.py`. M11 implements only
//! the Docker Engine API (HTTP-over-Unix-socket at `/var/run/docker.sock`).
//! Podman and LXD are intentionally PARTIAL and will follow later.
//!
//! Output: `Value::Array` of `Value::Object`, keyed by `id`. Each row:
//! id, name, engine, image, state, status, created, ports.
//!
//! Any failure (socket missing, permission denied, HTTP error, malformed
//! JSON) silently yields an empty array so the refresh loop never stalls.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;
use crate::plugins::json;

pub const NAME: &str = "containers";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(ContainersPlugin::new()));
}

pub const DOCKER_SOCK: &str = "/var/run/docker.sock";
pub const READ_TIMEOUT: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// Docker projection + collection
// ---------------------------------------------------------------------------

/// Project one parsed Docker container object onto our output shape.
pub fn project(obj: &BTreeMap<String, Value>) -> Value {
    let id = obj.get("Id").and_then(Value::as_str).unwrap_or("").to_string();
    // Docker /containers/json emits `Names` as ["/name"]. Strip the "/".
    let name = obj
        .get("Names")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_default();
    let image = obj.get("Image").and_then(Value::as_str).unwrap_or("").to_string();
    let state = obj.get("State").and_then(Value::as_str).unwrap_or("").to_string();
    let status = obj.get("Status").and_then(Value::as_str).unwrap_or("").to_string();
    let created = obj.get("Created").and_then(Value::as_i64).unwrap_or(0);
    let ports = obj.get("Ports").cloned().unwrap_or(Value::Null);

    let mut out = BTreeMap::new();
    out.insert("id".into(), Value::String(id));
    out.insert("name".into(), Value::String(name));
    out.insert("engine".into(), Value::String("docker".into()));
    out.insert("image".into(), Value::String(image));
    out.insert("state".into(), Value::String(state));
    out.insert("status".into(), Value::String(status));
    out.insert("created".into(), Value::Int(created));
    out.insert("ports".into(), ports);
    Value::Object(out)
}

/// Open the Docker socket, send `GET /containers/json`, return the
/// body of a 2xx response. Any failure returns None.
pub fn docker_request(sock: &str) -> Option<String> {
    let mut stream = UnixStream::connect(sock).ok()?;
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    // Two-line request exactly as the spec dictates.
    let req = "GET /containers/json HTTP/1.1\r\nHost: docker\r\n\r\n";
    stream.write_all(req.as_bytes()).ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    let sep = raw.find("\r\n\r\n")?;
    let head = &raw[..sep];
    let body = &raw[sep + 4..];
    if !head.starts_with("HTTP/1.") {
        return None;
    }
    let after_proto = &head[8..];
    let space = after_proto.find(' ')?;
    let code: u32 = after_proto[..space].parse().ok()?;
    if !(200..300).contains(&code) {
        return None;
    }
    Some(body.to_string())
}

/// Collect all containers. Always returns a Vec — empty on any failure.
pub fn collect(sock: &str) -> Vec<Value> {
    let body = match docker_request(sock) {
        Some(b) => b,
        None => return Vec::new(),
    };
    let arr = match json::parse_array(&body) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        if let Some(obj) = v.as_object() {
            out.push(project(obj));
        }
    }
    out
}

/// Re-export the shared parser entry point under a more descriptive name
/// (kept for backwards compatibility with any caller that imported it
/// directly from this module).
pub fn parse_json_array(input: &str) -> Option<Vec<Value>> {
    json::parse_array(input)
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct ContainersPlugin {
    base: GlancesPluginModel,
}

impl ContainersPlugin {
    pub fn new() -> Self {
        Self {
            base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())),
        }
    }
}

impl Default for ContainersPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for ContainersPlugin {
    fn name(&self) -> &'static str {
        NAME
    }
    fn reset(&mut self) {
        self.base.reset();
    }
    fn stats(&self) -> &Value {
        &self.base.stats
    }
    fn stats_mut(&mut self) -> &mut Value {
        &mut self.base.stats
    }
    fn get_key(&self) -> Option<&'static str> {
        Some("id")
    }

    fn update(&mut self) -> Result<()> {
        let arr = collect(DOCKER_SOCK);
        self.base.stats = Value::Array(arr);
        Ok(())
    }
}
