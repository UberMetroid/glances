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

/// Max bytes we will read from the Docker socket per request. A real
/// `/containers/json` response for hundreds of containers is well
/// under 4 MiB; beyond that we're looking at a hostile or broken peer.
const MAX_RESPONSE: usize = 4 * 1024 * 1024;

/// Open the Docker socket, send `GET /containers/json`, return the
/// body of a 2xx response. Any failure returns None.
///
/// Robustness contract: the read is bounded to MAX_RESPONSE bytes,
/// `Transfer-Encoding: chunked` bodies are dechunked, and the status
/// line is validated — a daemon misbehaving in any of these ways
/// yields `None`, never a stalled tick or unbounded allocation.
pub fn docker_request(sock: &str) -> Option<String> {
    let mut stream = UnixStream::connect(sock).ok()?;
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    // `Connection: close` is required: without it the daemon keeps the
    // HTTP/1.1 connection alive and the read blocks until the read
    // timeout — stalling every refresh tick and returning no data.
    let req = "GET /containers/json HTTP/1.1\r\nHost: docker\r\nConnection: close\r\n\r\n";
    stream.write_all(req.as_bytes()).ok()?;
    let raw = read_bounded(&mut stream, MAX_RESPONSE)?;
    let sep = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..sep]).into_owned();
    let body = &raw[sep + 4..];
    let mut head_lines = head.split("\r\n");
    let status = head_lines.next()?;
    if !status.starts_with("HTTP/") { return None; }
    let code: u32 = status.split_whitespace().nth(1)?.parse().ok()?;
    if !(200..300).contains(&code) { return None; }
    // Honor Transfer-Encoding: chunked — Docker may stream the list.
    let chunked = head_lines.any(|h| {
        let (k, v) = h.split_once(':').unwrap_or(("", ""));
        k.trim().eq_ignore_ascii_case("transfer-encoding")
            && v.split(',').any(|c| c.trim().eq_ignore_ascii_case("chunked"))
    });
    let body = if chunked { dechunk(body)? } else { body.to_vec() };
    String::from_utf8(body).ok()
}

/// Read until EOF or `cap` bytes; None on read error or cap exceeded.
fn read_bounded(stream: &mut UnixStream, cap: usize) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(64 * 1024);
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => return Some(buf),
            Ok(n) => {
                if buf.len() + n > cap { return None; }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(_) => return None,
        }
    }
}

/// Decode an HTTP chunked body. Returns the concatenated chunk data,
/// or None on malformed framing.
fn dechunk(raw: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(raw.len());
    let mut pos = 0usize;
    loop {
        let eol = raw[pos..].windows(2).position(|w| w == b"\r\n")? + pos;
        let size_str = std::str::from_utf8(&raw[pos..eol]).ok()?;
        // Strip optional chunk extensions (`;k=v`).
        let size = usize::from_str_radix(
            size_str.split(';').next()?.trim(), 16).ok()?;
        pos = eol + 2;
        if size == 0 { return Some(out); }
        if pos + size > raw.len() { return None; }
        out.extend_from_slice(&raw[pos..pos + size]);
        pos += size;
        if raw.get(pos..pos + 2) != Some(b"\r\n") { return None; }
        pos += 2;
    }
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
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value {
        &mut self.base.stats
    }
    fn history_items(&self) -> &[&'static str] { &["cpu_percent"] }
    fn get_key(&self) -> Option<&'static str> {
        // 'name' (not the hex 'id') — matches Python Glances' item key and
        // produces readable series names in exports.
        Some("name")
    }

    fn update(&mut self) -> Result<()> {
        let arr = collect(DOCKER_SOCK);
        self.base.stats = Value::Array(arr);
        Ok(())
    }
}
