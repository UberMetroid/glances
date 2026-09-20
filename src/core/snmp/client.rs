//! std-only SNMP client transport (UDP + `proto` codec).
//!
//! Mirrors `glances/snmp.py`: `get_by_oid` (one GET per call) and
//! `getbulk_by_oid` plus a `walk` helper for table MIBs. SNMPv1 and
//! v2c community auth only — v3 (USM/HMAC) is refused explicitly.
//! Every socket carries a read timeout so a dead agent fails fast
//! instead of hanging the refresh tick.

use std::net::UdpSocket;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use super::proto::{self};
use super::SnmpValue;
use crate::core::error::{GlancesError, Result};

static NEXT_ID: AtomicI64 = AtomicI64::new(0x1000);

fn next_id() -> i64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Which wire version to speak. v3 is refused at construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnmpProto {
    V1,
    V2c,
}

impl SnmpProto {
    fn version_int(self) -> i64 {
        match self {
            SnmpProto::V1 => 0,
            SnmpProto::V2c => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnmpClient {
    host: String,
    port: u16,
    proto: SnmpProto,
    community: String,
    timeout: Duration,
}

impl SnmpClient {
    pub fn new(host: &str, port: u16, version: &str, community: &str) -> Result<Self> {
        let proto = match version {
            "1" => SnmpProto::V1,
            "2c" | "2" => SnmpProto::V2c,
            "3" => {
                return Err(GlancesError::Other(
                    "SNMPv3 (USM auth) is not supported by this build; use 1 or 2c".into(),
                ));
            }
            other => {
                return Err(GlancesError::Other(format!(
                    "unknown SNMP version '{}'; want 1, 2c or 3",
                    other
                )));
            }
        };
        Ok(Self {
            host: host.to_string(),
            port,
            proto,
            community: community.to_string(),
            timeout: Duration::from_secs(3),
        })
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    fn round_trip(&self, msg: &[u8]) -> Result<Vec<u8>> {
        let sock = UdpSocket::bind("0.0.0.0:0").map_err(GlancesError::Io)?;
        sock.set_read_timeout(Some(self.timeout)).map_err(GlancesError::Io)?;
        sock.set_write_timeout(Some(self.timeout)).map_err(GlancesError::Io)?;
        let target = format!("{}:{}", self.host, self.port);
        sock.send_to(msg, &target).map_err(GlancesError::Io)?;
        let mut buf = vec![0u8; 65535];
        let (n, _) = sock.recv_from(&mut buf).map_err(GlancesError::Io)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Parse a RESPONSE PDU, checking the request id and error-status.
    fn parse_response(
        raw: &[u8],
        request_id: i64,
    ) -> Result<Vec<(String, SnmpValue)>> {
        let mut msg = proto::Reader::new(raw).seq().map_err(GlancesError::Parse)?;
        let (tag, _) = msg.tlv().map_err(GlancesError::Parse)?;
        if tag != 0x02 {
            return Err(GlancesError::Parse("SNMP: bad version field".into()));
        }
        let (tag, _) = msg.tlv().map_err(GlancesError::Parse)?;
        if tag != 0x04 {
            return Err(GlancesError::Parse("SNMP: bad community field".into()));
        }
        let (tag, body) = msg.tlv().map_err(GlancesError::Parse)?;
        if tag != 0xa2 {
            return Err(GlancesError::Parse(format!(
                "SNMP: not a response (tag {:#x})",
                tag
            )));
        }
        let mut pdu = proto::Reader::new(body);
        let (_, id_body) = pdu.tlv().map_err(GlancesError::Parse)?;
        let got: i64 = int_body(id_body)?;
        if got != request_id {
            return Err(GlancesError::Parse("SNMP: request-id mismatch".into()));
        }
        let (_, status_body) = pdu.tlv().map_err(GlancesError::Parse)?;
        let status = int_body(status_body)?;
        let _ = pdu.tlv().map_err(GlancesError::Parse)?; // error-index
        if status != 0 {
            return Err(GlancesError::Other(format!("SNMP agent error status {}", status)));
        }
        let mut vbl = pdu.seq().map_err(GlancesError::Parse)?;
        let mut out = Vec::new();
        while !vbl.is_empty() {
            let mut vb = vbl.seq().map_err(GlancesError::Parse)?;
            let (tag, name_body) = vb.tlv().map_err(GlancesError::Parse)?;
            if tag != 0x06 {
                return Err(GlancesError::Parse("SNMP: varbind without OID".into()));
            }
            let (vtag, vbody) = vb.tlv().map_err(GlancesError::Parse)?;
            out.push((
                proto::decode_oid(name_body).map_err(GlancesError::Parse)?,
                proto::decode_value(vtag, vbody).map_err(GlancesError::Parse)?,
            ));
        }
        Ok(out)
    }

    /// One GET request for a list of OIDs (upstream `get_by_oid`).
    pub fn get_by_oid(&self, oids: &[&str]) -> Result<Vec<(String, SnmpValue)>> {
        self.request(0xa0, 0, oids)
    }

    /// One GETNEXT request (used by v1 walks; v1 has no GETBULK).
    pub fn getnext_by_oid(&self, oids: &[&str]) -> Result<Vec<(String, SnmpValue)>> {
        self.request(0xa1, 0, oids)
    }

    fn request(&self, pdu_tag: u8, bulk_max: i64, oids: &[&str]) -> Result<Vec<(String, SnmpValue)>> {
        let bodies: Vec<Vec<u8>> = oids
            .iter()
            .map(|o| proto::encode_oid(o).map_err(GlancesError::Parse))
            .collect::<Result<_>>()?;
        let id = next_id();
        let msg = proto::build_message(
            self.proto.version_int(),
            &self.community,
            pdu_tag,
            id,
            bulk_max,
            &bodies,
        );
        let raw = self.round_trip(&msg)?;
        Self::parse_response(&raw, id)
    }

    /// One GETBULK request (v2c only; upstream `getbulk_by_oid`).
    pub fn getbulk_by_oid(
        &self,
        max_repetitions: u32,
        oids: &[&str],
    ) -> Result<Vec<(String, SnmpValue)>> {
        if self.proto != SnmpProto::V2c {
            return Err(GlancesError::Other(
                "GETBULK needs SNMPv2c (bulk is unavailable in v1)".into(),
            ));
        }
        let bodies: Vec<Vec<u8>> = oids
            .iter()
            .map(|o| proto::encode_oid(o).map_err(GlancesError::Parse))
            .collect::<Result<_>>()?;
        let id = next_id();
        let msg = proto::build_message(
            self.proto.version_int(),
            &self.community,
            0xa5,
            id,
            max_repetitions as i64,
            &bodies,
        );
        let raw = self.round_trip(&msg)?;
        Self::parse_response(&raw, id)
    }

    /// Walk a subtree with repeated GETBULK (v2c) or GETNEXT (v1).
    /// Stops at the first OID outside `base`, on endOfMibView, or
    /// after `limit` varbinds.
    pub fn walk(&self, base: &str, limit: usize) -> Result<Vec<(String, SnmpValue)>> {
        let prefix = format!("{}.", base.trim_end_matches('.'));
        let mut out = Vec::new();
        let mut next = base.to_string();
        while out.len() < limit {
            let batch = if self.proto == SnmpProto::V2c {
                self.getbulk_by_oid(25, &[next.as_str()])?
            } else {
                self.getnext_by_oid(&[next.as_str()])?
            };
            if batch.is_empty() {
                break;
            }
            let mut advanced = false;
            for (oid, value) in batch {
                if matches!(value, SnmpValue::Exception(_)) {
                    return Ok(out);
                }
                if !oid.starts_with(&prefix) && oid != base {
                    return Ok(out);
                }
                // For v1 the agent echoes Scalar OIDs; skip exact dupes.
                if out.last().is_some_and(|(o, _)| o == &oid) {
                    continue;
                }
                next = oid.clone();
                out.push((oid, value));
                advanced = true;
            }
            if !advanced {
                break;
            }
        }
        Ok(out)
    }
}

fn int_body(body: &[u8]) -> Result<i64> {
    if body.is_empty() || body.len() > 8 {
        return Err(GlancesError::Parse("SNMP: bad INTEGER".into()));
    }
    let mut v: i64 = if body[0] & 0x80 != 0 { -1 } else { 0 };
    for b in body {
        v = (v << 8) | *b as i64;
    }
    Ok(v)
}
