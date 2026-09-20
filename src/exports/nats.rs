//! NATS text-protocol exporter — sends `PUB <subject> <#bytes>\r\n<payload>\r\n`
//! over a TCP connection. NATS is server-initiated (broker sends INFO first),
//! but the client PUB command itself is text-based and can be sent without
//! waiting for INFO. Brokers that require TLS / auth are out of scope.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "nats";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub subject_prefix: String,
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 4222,
            subject_prefix: "glances".into(),
            timeout_secs: 5,
        }
    }
}

/// Subject sanitize: spaces, CR/LF and the wildcard tokens `*`/`>` are
/// illegal or protocol-active in NATS subjects — map to `_`.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' | '\t' | '\r' | '\n' | '\0'..='\x1f' | '*' | '>' => '_',
            c => c,
        })
        .collect()
}

/// Build one `PUB subject payload-size CR LF payload CR LF` command
/// (no reply-to). Exposed for unit tests.
pub fn build_pub(prefix: &str, plugin: &str, key: &str, payload: &[u8]) -> Vec<u8> {
    let subject = format!("{}.{}.{}", sanitize(prefix), sanitize(plugin), sanitize(key));
    let mut out = Vec::with_capacity(payload.len() + subject.len() + 32);
    out.extend_from_slice(b"PUB ");
    out.extend_from_slice(subject.as_bytes());
    out.push(b' ');
    out.extend_from_slice(payload.len().to_string().as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\r\n");
    out
}

fn render_payload(value: &Value) -> Vec<u8> {
    match value {
        Value::Int(i) => i.to_string().into_bytes(),
        Value::Uint(u) => u.to_string().into_bytes(),
        Value::Float(f) if f.is_nan() || f.is_infinite() => b"null".to_vec(),
        Value::Float(f) => format!("{}", f).into_bytes(),
        Value::Bool(b) => b.to_string().into_bytes(),
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Null | Value::Array(_) | Value::Object(_) => b"null".to_vec(),
    }
}

pub fn build_publishes(fields: &[Field<'_>], prefix: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for f in fields {
        let payload = render_payload(f.value);
        out.extend_from_slice(&build_pub(prefix, &f.series, f.key, &payload));
    }
    out
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    if cfg.subject_prefix.is_empty() {
        return Err(GlancesError::InvalidConfig(
            "nats exporter requires subject_prefix".into(),
        ));
    }
    let body = build_publishes(fields, &cfg.subject_prefix);
    if body.is_empty() { return Ok(()); }

    let mut addr_iter = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let addr = addr_iter.next().ok_or_else(|| {
        GlancesError::InvalidConfig(format!("no addresses for {}", cfg.host))
    })?;

    let timeout = Duration::from_secs(cfg.timeout_secs);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn obj(pairs: &[(&str, Value)]) -> Value {
        let mut m = BTreeMap::new();
        for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
        Value::Object(m)
    }

    #[test]
    fn pub_command_has_subject_and_payload_size() {
        let cmd = build_pub("glances", "cpu", "total", b"42");
        let raw = String::from_utf8(cmd).unwrap();
        // No reply-to: `PUB <subject> <size>\r\n<payload>\r\n` exactly.
        assert_eq!(raw, "PUB glances.cpu.total 2\r\n42\r\n");
    }

    #[test]
    fn pub_handles_empty_payload() {
        let cmd = build_pub("p", "a", "b", b"");
        let raw = String::from_utf8(cmd).unwrap();
        assert_eq!(raw, "PUB p.a.b 0\r\n\r\n");
    }

    #[test]
    fn publish_set_emits_one_pub_per_field() {
        let snap = obj(&[(
            "cpu",
            obj(&[("x", Value::Int(1)), ("y", Value::Int(2))]),
        )]);
        let fields = crate::exports::flatten::collect(&snap, &Default::default());
        let bytes = build_publishes(&fields, "gl");
        let raw = String::from_utf8(bytes).unwrap();
        assert!(raw.contains("PUB gl.cpu.x 1\r\n1\r\n"));
        assert!(raw.contains("PUB gl.cpu.y 1\r\n2\r\n"));
    }

    #[test]
    fn subject_sanitizes_wildcards_and_whitespace() {
        let cmd = build_pub("g l", "cp*u", "to>tal", b"1");
        let raw = String::from_utf8(cmd).unwrap();
        assert_eq!(raw, "PUB g_l.cp_u.to_tal 1\r\n1\r\n");
    }

    #[test]
    fn empty_prefix_rejected() {
        let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
        let cfg = Config { subject_prefix: String::new(), ..Default::default() };
        let fields = crate::exports::flatten::collect(&snap, &Default::default());
        let err = write(&fields, &cfg).unwrap_err();
        assert!(matches!(err, GlancesError::InvalidConfig(_)));
    }
}