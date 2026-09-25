//! SNMP transport: community-auth UDP with the `proto` codec.
//!
//! One GET/GETNEXT/GETBULK per call plus a subtree walker. Every socket
//! carries timeouts so a dead agent fails fast instead of hanging the
//! refresh tick.

use std::net::UdpSocket;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use super::proto;
use super::SnmpValue;
use crate::core::error::{GlancesError, Result};

static NEXT_ID: AtomicI64 = AtomicI64::new(0x1000);

fn next_id() -> i64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Speakable wire versions (v3 refuses at construction).
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
                    "unknown SNMP version '{other}'; want 1, 2c or 3"
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
        sock.send_to(msg, target.as_str()).map_err(GlancesError::Io)?;
        let mut buf = vec![0u8; 65535];
        let (n, _) = sock.recv_from(&mut buf).map_err(GlancesError::Io)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Decode a RESPONSE: envelope tags, echoed request id, zero error
    /// status, then (OID, value) pairs.
    fn parse_response(raw: &[u8], request_id: i64) -> Result<Vec<(String, SnmpValue)>> {
        let mut msg = proto::Reader::new(raw).seq().map_err(GlancesError::Parse)?;
        expect_tag(&mut msg, 0x02, "SNMP: bad version field")?;
        expect_tag(&mut msg, 0x04, "SNMP: bad community field")?;
        let (tag, body) = msg.tlv().map_err(GlancesError::Parse)?;
        if tag != 0xa2 {
            return Err(GlancesError::Parse(format!("SNMP: not a response (tag {tag:#x})")));
        }
        let mut pdu = proto::Reader::new(body);
        let (_, id_raw) = pdu.tlv().map_err(GlancesError::Parse)?;
        if decode_int(id_raw)? != request_id {
            return Err(GlancesError::Parse("SNMP: request-id mismatch".into()));
        }
        let (_, status_raw) = pdu.tlv().map_err(GlancesError::Parse)?;
        let status = decode_int(status_raw)?;
        if status != 0 {
            return Err(GlancesError::Other(format!("SNMP agent error status {status}")));
        }
        let _ = pdu.tlv().map_err(GlancesError::Parse)?;
        let mut vbl = pdu.seq().map_err(GlancesError::Parse)?;
        let mut out = Vec::new();
        while !vbl.is_empty() {
            let mut vb = vbl.seq().map_err(GlancesError::Parse)?;
            let (tag, name_raw) = vb.tlv().map_err(GlancesError::Parse)?;
            if tag != 0x06 {
                return Err(GlancesError::Parse("SNMP: varbind without OID".into()));
            }
            let (vtag, vraw) = vb.tlv().map_err(GlancesError::Parse)?;
            out.push((
                proto::decode_oid(name_raw).map_err(GlancesError::Parse)?,
                proto::decode_value(vtag, vraw).map_err(GlancesError::Parse)?,
            ));
        }
        Ok(out)
    }

    /// One GET over a list of OIDs.
    pub fn get_by_oid(&self, oids: &[&str]) -> Result<Vec<(String, SnmpValue)>> {
        self.exchange(0xa0, 0, oids)
    }

    /// One GETNEXT (v1 walks use this; v1 has no GETBULK).
    pub fn getnext_by_oid(&self, oids: &[&str]) -> Result<Vec<(String, SnmpValue)>> {
        self.exchange(0xa1, 0, oids)
    }

    /// One GETBULK (v2c only).
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
        self.exchange(0xa5, max_repetitions as i64, oids)
    }

    fn exchange(&self, pdu_tag: u8, bulk_max: i64, oids: &[&str]) -> Result<Vec<(String, SnmpValue)>> {
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
        Self::parse_response(&self.round_trip(&msg)?, id)
    }

    /// Walk a subtree: repeated GETBULK on v2c, GETNEXT on v1. Stops
    /// outside `base`, on exception markers, on stalled progress, or at
    /// `limit` varbinds.
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
            let mut moved = false;
            for (oid, value) in batch {
                if matches!(value, SnmpValue::Exception(_)) {
                    return Ok(out);
                }
                if oid != base && !oid.starts_with(&prefix) {
                    return Ok(out);
                }
                // v1 agents echo scalar OIDs back; skip exact duplicates.
                if out.last().is_some_and(|(o, _)| o == &oid) {
                    continue;
                }
                next.clone_from(&oid);
                out.push((oid, value));
                moved = true;
            }
            if !moved {
                break;
            }
        }
        Ok(out)
    }
}

fn expect_tag(r: &mut proto::Reader<'_>, want: u8, err: &str) -> Result<()> {
    let (tag, _) = r.tlv().map_err(GlancesError::Parse)?;
    if tag == want {
        Ok(())
    } else {
        Err(GlancesError::Parse(err.into()))
    }
}

fn decode_int(body: &[u8]) -> Result<i64> {
    if body.is_empty() || body.len() > 8 {
        return Err(GlancesError::Parse("SNMP: bad INTEGER".into()));
    }
    let mut v: i64 = if body[0] & 0x80 != 0 { -1 } else { 0 };
    for b in body {
        v = (v << 8) | *b as i64;
    }
    Ok(v)
}
