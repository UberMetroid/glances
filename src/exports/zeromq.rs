//! ZeroMQ PUB exporter — binds `tcp://host:port` and broadcasts one
//! three-frame message per plugin (`[prefix, plugin, json]`).
//!
//! Mirrors `glances/exports/glances_zeromq/__init__.py` (pyzmq PUB that
//! binds). The ZMTP 3.0 NULL-mechanism handshake is implemented directly
//! (greeting + READY exchange) so no `libzmq` is needed.
//!
//! Simplification vs upstream: every connected peer receives every
//! message (no server-side subscription filtering). SUB peers still
//! filter client-side, which is where correctness lives in ZMTP.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::{self, Value};
use crate::exports::flatten::Field;

pub const NAME: &str = "zeromq";

/// Default ZeroMQ PUB port.
pub const DEFAULT_PORT: u16 = 5555;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub prefix: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: DEFAULT_PORT,
            prefix: "glances".into(),
            timeout_secs: 5,
        }
    }
}

/// ZMTP 3.0 greeting, server side: 10-octet signature (`FF 00*8 7F`),
/// version 3.0, NULL mechanism (20 octets), as-server flag, zero filler.
pub fn greeting() -> [u8; 64] {
    let mut g = [0u8; 64];
    g[0] = 0xFF;
    g[9] = 0x7F;
    g[10] = 3;
    g[11] = 0;
    g[12..16].copy_from_slice(b"NULL");
    g[32] = 1;
    g
}

/// Validate a peer greeting: signature, version 3.x, NULL mechanism.
pub fn valid_greeting(g: &[u8]) -> bool {
    if g.len() != 64 || g[0] != 0xFF || g[9] != 0x7F || g[10] != 3 {
        return false;
    }
    &g[12..32] == b"NULL\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"
}

/// Encode one READY command with a single `Socket-Type` property.
pub fn ready_command(socket_type: &str) -> Vec<u8> {
    let mut body = vec![5u8, b'R', b'E', b'A', b'D', b'Y'];
    let name = b"Socket-Type";
    body.push(name.len() as u8);
    body.extend_from_slice(name);
    let val = socket_type.as_bytes();
    body.extend_from_slice(&(val.len() as u32).to_be_bytes());
    body.extend_from_slice(val);
    let mut out = vec![0x04];
    out.push(body.len() as u8);
    out.extend_from_slice(&body);
    out
}

/// Encode one message frame (`more` sets the MORE flag). Payloads over
/// 255 bytes use the long form.
pub fn encode_frame(data: &[u8], more: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 9);
    if data.len() > 255 {
        out.push(if more { 0x03 } else { 0x02 });
        out.extend_from_slice(&(data.len() as u64).to_be_bytes());
    } else {
        out.push(if more { 0x01 } else { 0x00 });
        out.push(data.len() as u8);
    }
    out.extend_from_slice(data);
    out
}

/// Perform the server side of the NULL handshake on a fresh peer.
/// Returns the stream ready for message frames.
pub fn handshake(mut s: TcpStream, timeout: Duration) -> Result<TcpStream> {
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    let mut g = [0u8; 64];
    s.read_exact(&mut g)?;
    if !valid_greeting(&g) {
        return Err(GlancesError::Parse("zmq: bad peer greeting".into()));
    }
    s.write_all(&greeting())?;
    s.flush()?;
    // Peer READY command (short or long form) — consumed, not filtered on.
    let mut flag = [0u8; 1];
    s.read_exact(&mut flag)?;
    let size = if flag[0] & 0x02 == 0 {
        let mut n = [0u8; 1];
        s.read_exact(&mut n)?;
        n[0] as usize
    } else {
        let mut n = [0u8; 8];
        s.read_exact(&mut n)?;
        u64::from_be_bytes(n) as usize
    };
    let mut cmd = vec![0u8; size];
    s.read_exact(&mut cmd)?;
    // Our READY: this side is the publisher.
    s.write_all(&ready_command("PUB"))?;
    s.flush()?;
    Ok(s)
}

/// Connected-peer set for the bound publisher.
pub struct Publisher {
    bound: bool,
    pub peers: Vec<TcpStream>,
}

impl Publisher {
    pub fn new() -> Self {
        Self { bound: false, peers: Vec::new() }
    }

    /// Bind once and spawn the accept thread; later calls are no-ops.
    pub fn ensure_bound(&mut self, cfg: &Config) -> Result<()> {
        if self.bound {
            return Ok(());
        }
        let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
        let addr = addr_iter.next().ok_or_else(|| {
            GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
        })?;
        let listener = TcpListener::bind(addr)?;
        let timeout = Duration::from_secs(cfg.timeout_secs);
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if let Ok(s) = conn {
                    if let Ok(ready) = handshake(s, timeout) {
                        if let Ok(mut guard) = publisher().lock() {
                            guard.peers.push(ready);
                        }
                    }
                }
            }
        });
        self.bound = true;
        Ok(())
    }

    /// Broadcast one three-frame message to every peer, pruning dead ones.
    pub fn broadcast(&mut self, prefix: &str, plugin: &str, payload: &[u8]) {
        let mut msg = encode_frame(prefix.as_bytes(), true);
        msg.extend_from_slice(&encode_frame(plugin.as_bytes(), true));
        msg.extend_from_slice(&encode_frame(payload, false));
        let mut alive = Vec::new();
        for mut p in std::mem::take(&mut self.peers) {
            let mut ok = p.write_all(&msg).is_ok();
            ok = ok && p.flush().is_ok();
            if ok {
                alive.push(p);
            }
        }
        self.peers = alive;
    }
}

fn publisher() -> &'static Mutex<Publisher> {
    static P: OnceLock<Mutex<Publisher>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(Publisher::new()))
}

/// Group flattened fields by plugin into (name, json-payload) pairs.
pub fn group_payloads(fields: &[Field<'_>]) -> Vec<(String, String)> {
    let mut order: Vec<&str> = Vec::new();
    let mut groups: BTreeMap<&str, BTreeMap<String, Value>> = BTreeMap::new();
    for f in fields {
        if !order.contains(&f.plugin) {
            order.push(f.plugin);
        }
        let key = match &f.elem {
            Some(e) => format!("{}.{}", e, f.key),
            None => f.key.to_string(),
        };
        groups.entry(f.plugin).or_default().insert(key, (*f.value).clone());
    }
    order
        .into_iter()
        .map(|plugin| {
            let obj = groups.remove(plugin).unwrap_or_default();
            (plugin.to_string(), value::to_json(&Value::Object(obj)))
        })
        .collect()
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let payloads = group_payloads(fields);
    if payloads.is_empty() {
        return Ok(());
    }
    let mut guard = publisher().lock().unwrap_or_else(|e| e.into_inner());
    guard.ensure_bound(cfg)?;
    for (plugin, json) in &payloads {
        guard.broadcast(&cfg.prefix, plugin, json.as_bytes());
    }
    Ok(())
}
