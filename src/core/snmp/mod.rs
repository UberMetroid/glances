//! std-only SNMP client (upstream `glances/snmp.py` parity).
//!
//! v1 / v2c community auth over UDP: `GET`, `GETNEXT`, `GETBULK`,
//! subtree `walk`, and the `check_snmp` probe (sysName reachability
//! + sysDescr OS mapping). SNMPv3/USM is refused explicitly —
//!   upstream needs pysnmp for it and there is no std HMAC/MD5.

pub mod client;
pub mod proto;

pub use client::{SnmpClient, SnmpProto};

use crate::core::error::{GlancesError, Result};

/// SNMP value decoded from a VarBind.
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

/// Upstream `oid_to_short_system_name`, order-preserved. Matching is
/// case-insensitive substring (upstream uses `.*X.*` regexes).
const OS_MAP: &[(&str, &str)] = &[
    ("linux", "linux"),
    ("darwin", "mac"),
    ("bsd", "bsd"),
    ("windows", "windows"),
    ("cisco", "cisco"),
    ("vmware esxi", "esxi"),
    ("netapp", "netapp"),
];

/// Map a sysDescr string to the short OS name (upstream
/// `GlancesStatsClientSNMP.get_system_name` parity).
pub fn system_name(sys_descr: &str) -> Option<String> {
    if sys_descr.is_empty() {
        return None;
    }
    let lower = sys_descr.to_ascii_lowercase();
    OS_MAP
        .iter()
        .find(|(pat, _)| lower.contains(pat))
        .map(|(_, short)| short.to_string())
}

/// Bulk GET for `key → OID` pairs (upstream `get_stats_snmp`
/// parity). Returns only the pairs the agent answered; callers
/// treat a missing key as "no data" and reset, like upstream's
/// empty-string check.
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

/// Per-tick SNMP context handed to plugins (host connection +
/// detected OS family, like upstream `input_method`/`short_system_name`).
#[derive(Debug, Clone)]
pub struct SnmpCtx {
    pub client: SnmpClient,
    pub system_name: Option<String>,
}

impl SnmpCtx {
    /// Probe the agent: sysName must answer (upstream `check_snmp`),
    /// then sysDescr picks the OS family (missing → `None`, like
    /// upstream's "Cannot detect" warning path).
    pub fn probe(client: &SnmpClient) -> Result<Self> {
        let name = client.get_by_oid(&[OID_SYS_NAME])?;
        let hostname = name
            .first()
            .and_then(|(_, v)| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| GlancesError::Other("SNMP: no sysName answer".into()))?;
        let _ = hostname;
        let descr = client
            .get_by_oid(&[OID_SYS_DESCR])
            .ok()
            .and_then(|r| r.into_iter().next())
            .and_then(|(_, v)| v.as_str().map(|s| s.to_string()));
        Ok(Self {
            client: client.clone(),
            system_name: descr.as_deref().and_then(system_name),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::proto::{decode_oid, encode_oid};
    use super::{system_name, SnmpClient};
    use std::time::Duration;

    #[test]
    fn oid_known_answer() {
        // sysName OID (RFC 1213): first two arcs collapse to 0x2b.
        assert_eq!(
            encode_oid("1.3.6.1.2.1.1.5.0").unwrap(),
            vec![0x2b, 0x06, 0x01, 0x02, 0x01, 0x01, 0x05, 0x00]
        );
    }

    #[test]
    fn oid_round_trip() {
        for oid in ["1.3.6.1.2.1.1.5.0", "1.3.6.1.4.1.2021.11.9.0", "1.3.6.1.2.1.25.3.3.1.2.1"] {
            let enc = encode_oid(oid).unwrap();
            assert_eq!(decode_oid(&enc).unwrap(), oid);
        }
    }

    #[test]
    fn oid_rejects_garbage() {
        assert!(encode_oid("1.3.6.x.0").is_err());
        assert!(encode_oid("1").is_err());
        assert!(decode_oid(&[]).is_err());
    }

    #[test]
    fn os_map_matches_upstream_order() {
        assert_eq!(system_name("Linux server 6.8.0 x86_64").as_deref(), Some("linux"));
        assert_eq!(system_name("Darwin Kernel 23").as_deref(), Some("mac"));
        assert_eq!(system_name("VMware ESXi 8.0").as_deref(), Some("esxi"));
        assert_eq!(system_name("Cisco IOS").as_deref(), Some("cisco"));
        assert_eq!(system_name(""), None);
        assert_eq!(system_name("Something Unknown 1.0"), None);
    }

    #[test]
    fn v3_refused() {
        assert!(SnmpClient::new("127.0.0.1", 161, "3", "public").is_err());
        assert!(SnmpClient::new("127.0.0.1", 161, "9", "public").is_err());
        assert!(SnmpClient::new("127.0.0.1", 161, "1", "public").is_ok());
        assert!(SnmpClient::new("127.0.0.1", 161, "2c", "public").is_ok());
    }

    /// Loopback fake agent: answers one GET for sysName with
    /// "testbox", echoing the request id. Proves the request
    /// builder and response parser agree on the wire format.
    #[test]
    fn loopback_get_round_trip() {
        use super::proto::Reader;
        use std::net::UdpSocket;

        let agent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let agent_port = agent.local_addr().unwrap().port();
        let responder = std::thread::spawn(move || {
            let mut buf = vec![0u8; 65535];
            let (n, from) = agent.recv_from(&mut buf).unwrap();
            // Extract the request id: seq{ int, str, pdu{ int,... } }.
            let mut msg = Reader::new(&buf[..n]).seq().unwrap();
            let _ = msg.tlv().unwrap();
            let _ = msg.tlv().unwrap();
            let (_, pdu) = msg.tlv().unwrap();
            let mut pdu = Reader::new(pdu);
            let (_, id_body) = pdu.tlv().unwrap();
            let mut id: i64 = if id_body[0] & 0x80 != 0 { -1 } else { 0 };
            for b in id_body {
                id = (id << 8) | *b as i64;
            }
            // RESPONSE: sysName.0 = "testbox".
            let mut vb = Vec::new();
            vb.push(0x06);
            vb.push(0x08);
            vb.extend_from_slice(&[0x2b, 0x06, 0x01, 0x02, 0x01, 0x01, 0x05, 0x00]);
            let name = b"testbox";
            vb.push(0x04);
            vb.push(name.len() as u8);
            vb.extend_from_slice(name);
            let mut vb_seq = Vec::new();
            vb_seq.push(0x30);
            vb_seq.push(vb.len() as u8);
            vb_seq.extend_from_slice(&vb);
            let mut vbl = Vec::new();
            vbl.push(0x30);
            vbl.push(vb_seq.len() as u8);
            vbl.extend_from_slice(&vb_seq);
            let enc_int = |v: i64, out: &mut Vec<u8>| {
                out.push(0x02);
                if v < 128 {
                    out.push(1);
                    out.push(v as u8);
                } else {
                    out.push(2);
                    out.push((v >> 8) as u8);
                    out.push(v as u8);
                }
            };
            let mut body = Vec::new();
            enc_int(id, &mut body);
            enc_int(0, &mut body);
            enc_int(0, &mut body);
            body.extend_from_slice(&vbl);
            let mut pdu_out = Vec::new();
            pdu_out.push(0xa2);
            pdu_out.push(body.len() as u8);
            pdu_out.extend_from_slice(&body);
            // Rebuild the outer message manually around the real PDU.
            let mut msg_body = Vec::new();
            enc_int(1, &mut msg_body);
            msg_body.push(0x04);
            msg_body.push(6);
            msg_body.extend_from_slice(b"public");
            msg_body.extend_from_slice(&pdu_out);
            let mut out = Vec::new();
            out.push(0x30);
            out.push(msg_body.len() as u8);
            out.extend_from_slice(&msg_body);
            agent.send_to(&out, from).unwrap();
        });
        let mut client = SnmpClient::new("127.0.0.1", agent_port, "2c", "public").unwrap();
        client.set_timeout(Duration::from_secs(5));
        let ans = client.get_by_oid(&["1.3.6.1.2.1.1.5.0"]).unwrap();
        responder.join().unwrap();
        assert_eq!(ans.len(), 1);
        assert_eq!(ans[0].0, "1.3.6.1.2.1.1.5.0");
        assert_eq!(ans[0].1.as_str(), Some("testbox"));
    }
}
