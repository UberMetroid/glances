//! SNMP agent model: decoded values, OS detection, bulk reads.
//!
//! Community auth (v1/v2c) over UDP. SNMPv3 needs USM/HMAC machinery
//! outside std, so it is refused explicitly at construction.

pub mod client;
pub mod proto;

pub use client::{SnmpClient, SnmpProto};

use crate::core::error::{GlancesError, Result};

/// A decoded variable binding.
#[derive(Debug, Clone, PartialEq)]
pub enum SnmpValue {
    Int(i64),
    Uint(u64),
    Str(String),
    Ip([u8; 4]),
    Null,
    /// noSuchObject / noSuchInstance / endOfMibView marker.
    Exception(u8),
}

impl SnmpValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            SnmpValue::Int(n) => Some(*n as f64),
            SnmpValue::Uint(n) => Some(*n as f64),
            SnmpValue::Str(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            SnmpValue::Str(s) => Some(s),
            _ => None,
        }
    }
}

pub const OID_SYS_DESCR: &str = "1.3.6.1.2.1.1.1.0";
pub const OID_SYS_NAME: &str = "1.3.6.1.2.1.1.5.0";

/// sysDescr substring → short OS name, first hit wins (order matters).
const OS_MAP: &[(&str, &str)] = &[
    ("linux", "linux"),
    ("darwin", "mac"),
    ("bsd", "bsd"),
    ("windows", "windows"),
    ("cisco", "cisco"),
    ("vmware esxi", "esxi"),
    ("netapp", "netapp"),
];

/// Short OS name from a sysDescr string (case-insensitive substring).
/// Empty and unrecognized descriptions yield None.
pub fn system_name(sys_descr: &str) -> Option<String> {
    if sys_descr.is_empty() {
        return None;
    }
    let lower = sys_descr.to_ascii_lowercase();
    OS_MAP.iter().find(|(pat, _)| lower.contains(pat)).map(|(_, short)| short.to_string())
}

/// Read a batch of `key → OID` pairs. Only answered pairs come back;
/// callers treat a missing key as "no data".
pub fn get_map(
    client: &SnmpClient,
    pairs: &[(&str, &str)],
) -> Result<std::collections::BTreeMap<String, SnmpValue>> {
    let oids: Vec<&str> = pairs.iter().map(|(_, o)| *o).collect();
    let ans = client.get_by_oid(&oids)?;
    let mut out = std::collections::BTreeMap::new();
    for (key, oid) in pairs {
        if let Some((_, v)) = ans.iter().find(|(o, _)| o == oid) {
            out.insert(key.to_string(), v.clone());
        }
    }
    Ok(out)
}

/// Per-tick agent context: the connection plus the detected OS family.
#[derive(Debug, Clone)]
pub struct SnmpCtx {
    pub client: SnmpClient,
    pub system_name: Option<String>,
}

impl SnmpCtx {
    /// Probe reachability: sysName must answer non-empty, then sysDescr
    /// picks the OS family best-effort (missing stays None).
    pub fn probe(client: &SnmpClient) -> Result<Self> {
        let answered = client.get_by_oid(&[OID_SYS_NAME])?;
        let live = answered
            .first()
            .and_then(|(_, v)| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| GlancesError::Other("SNMP: no sysName answer".into()))?;
        let _ = live;
        let descr = client
            .get_by_oid(&[OID_SYS_DESCR])
            .ok()
            .and_then(|r| r.into_iter().next())
            .and_then(|(_, v)| v.as_str().map(str::to_string));
        Ok(Self { client: client.clone(), system_name: descr.as_deref().and_then(system_name) })
    }
}

#[cfg(test)]
mod tests {
    use super::proto::{decode_oid, encode_oid};
    use super::{system_name, SnmpClient};
    use std::time::Duration;

    #[test]
    fn sysname_oid_bytes() {
        // RFC 1213: arcs 1.3 collapse to the single byte 0x2b.
        assert_eq!(
            encode_oid("1.3.6.1.2.1.1.5.0").unwrap(),
            vec![0x2b, 0x06, 0x01, 0x02, 0x01, 0x01, 0x05, 0x00]
        );
    }

    #[test]
    fn oid_text_survives_a_round_trip() {
        for oid in ["1.3.6.1.2.1.1.5.0", "1.3.6.1.4.1.2021.11.9.0", "1.3.6.1.2.1.25.3.3.1.2.1"] {
            assert_eq!(decode_oid(&encode_oid(oid).unwrap()).unwrap(), oid);
        }
        assert!(encode_oid("1.3.6.x.0").is_err());
        assert!(encode_oid("1").is_err());
        assert!(decode_oid(&[]).is_err());
    }

    #[test]
    fn os_detection_is_ordered_and_partial() {
        assert_eq!(system_name("Linux server 6.8.0").as_deref(), Some("linux"));
        assert_eq!(system_name("Darwin Kernel 23").as_deref(), Some("mac"));
        assert_eq!(system_name("VMware ESXi 8.0").as_deref(), Some("esxi"));
        assert_eq!(system_name("Cisco IOS").as_deref(), Some("cisco"));
        assert_eq!(system_name(""), None);
        assert_eq!(system_name("Something Unknown 1.0"), None);
    }

    #[test]
    fn only_v1_and_v2c_construct() {
        assert!(SnmpClient::new("127.0.0.1", 161, "3", "public").is_err());
        assert!(SnmpClient::new("127.0.0.1", 161, "9", "public").is_err());
        assert!(SnmpClient::new("127.0.0.1", 161, "1", "public").is_ok());
        assert!(SnmpClient::new("127.0.0.1", 161, "2c", "public").is_ok());
    }

    fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![tag, body.len() as u8];
        v.extend_from_slice(body);
        v
    }

    fn int_tlv(v: i64) -> Vec<u8> {
        let bytes = if v < 128 { vec![v as u8] } else { vec![(v >> 8) as u8, v as u8] };
        tlv(0x02, &bytes)
    }

    /// Loopback fake agent answering one sysName GET with "testbox":
    /// proves the request builder and the response parser agree on the
    /// wire format without needing a real agent.
    #[test]
    fn loopback_agent_round_trip() {
        use super::proto::Reader;
        use std::net::UdpSocket;

        let agent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = agent.local_addr().unwrap().port();
        let responder = std::thread::spawn(move || {
            let mut buf = vec![0u8; 65535];
            let (n, from) = agent.recv_from(&mut buf).unwrap();
            // Request id path: seq{ int, str, pdu{ int, ... } }.
            let mut msg = Reader::new(&buf[..n]).seq().unwrap();
            let _ = msg.tlv().unwrap();
            let _ = msg.tlv().unwrap();
            let (_, pdu) = msg.tlv().unwrap();
            let mut pdu_r = Reader::new(pdu);
            let (_, id_raw) = pdu_r.tlv().unwrap();
            let mut id: i64 = if id_raw[0] & 0x80 != 0 { -1 } else { 0 };
            for b in id_raw {
                id = (id << 8) | *b as i64;
            }
            // RESPONSE PDU: sysName.0 = "testbox".
            let vb = [tlv(0x06, &[0x2b, 0x06, 0x01, 0x02, 0x01, 0x01, 0x05, 0x00]), tlv(0x04, b"testbox")].concat();
            let vbl = tlv(0x30, &tlv(0x30, &vb));
            let mut body = int_tlv(id);
            body.extend_from_slice(&int_tlv(0));
            body.extend_from_slice(&int_tlv(0));
            body.extend_from_slice(&vbl);
            let pdu_out = tlv(0xa2, &body);
            let mut inner = int_tlv(1);
            inner.extend_from_slice(&tlv(0x04, b"public"));
            inner.extend_from_slice(&pdu_out);
            agent.send_to(&tlv(0x30, &inner), from).unwrap();
        });
        let mut client = SnmpClient::new("127.0.0.1", port, "2c", "public").unwrap();
        client.set_timeout(Duration::from_secs(5));
        let ans = client.get_by_oid(&["1.3.6.1.2.1.1.5.0"]).unwrap();
        responder.join().unwrap();
        assert_eq!(ans.len(), 1);
        assert_eq!(ans[0].0, "1.3.6.1.2.1.1.5.0");
        assert_eq!(ans[0].1.as_str(), Some("testbox"));
    }
}
