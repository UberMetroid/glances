//! SNMP client-mode conformance: a loopback fake agent serving a
//! small UCD+MIB-II database, then a full `update_snmp` tick whose
//! plugin shapes must match the local-update key contract.

use std::collections::HashMap;
use std::net::UdpSocket;
use std::time::Duration;

use crate::core::snmp::{SnmpClient, SnmpCtx};
use crate::core::stats::GlancesStats;
use crate::plugins;

/// One database value: (BER tag, contents).
fn db() -> HashMap<String, (u8, Vec<u8>)> {
    let mut m = HashMap::new();
    let s = |x: &str| (0x04u8, x.as_bytes().to_vec());
    let i = |v: i64| (0x02u8, vec![v as u8]);
    let t = |v: u32| (0x43u8, v.to_be_bytes().to_vec());
    m.insert("1.3.6.1.2.1.1.1.0".into(), s("Linux testbox 6.8.0 x86_64"));
    m.insert("1.3.6.1.2.1.1.3.0".into(), t(12_345_600));
    m.insert("1.3.6.1.2.1.1.5.0".into(), s("testbox"));
    m.insert("1.3.6.1.4.1.2021.10.1.3.1".into(), s("0.08"));
    m.insert("1.3.6.1.4.1.2021.10.1.3.2".into(), s("0.05"));
    m.insert("1.3.6.1.4.1.2021.10.1.3.3".into(), s("0.01"));
    m.insert("1.3.6.1.4.1.2021.11.9.0".into(), i(12));
    m.insert("1.3.6.1.4.1.2021.11.10.0".into(), i(5));
    m.insert("1.3.6.1.4.1.2021.11.11.0".into(), i(83));
    m.insert("1.3.6.1.4.1.2021.4.5.0".into(), i(100));
    m.insert("1.3.6.1.4.1.2021.4.11.0".into(), i(40));
    m.insert("1.3.6.1.4.1.2021.4.13.0".into(), i(2));
    m.insert("1.3.6.1.4.1.2021.4.14.0".into(), i(4));
    m.insert("1.3.6.1.4.1.2021.4.15.0".into(), i(10));
    m
}

fn arcs(oid: &str) -> Vec<u64> {
    oid.split('.').filter_map(|p| p.parse().ok()).collect()
}

fn enc_oid(oid: &str) -> Vec<u8> {
    crate::core::snmp::proto::encode_oid(oid).unwrap()
}

fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    if body.len() < 128 {
        out.push(body.len() as u8);
    } else {
        out.push(0x81);
        out.push(body.len() as u8);
    }
    out.extend_from_slice(body);
    out
}

/// Minimal BER reader for the fake agent (requests only).
struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        s
    }
    fn tlv(&mut self) -> (u8, &'a [u8]) {
        let tag = self.take(1)[0];
        let b = self.take(1)[0];
        let len = if b & 0x80 == 0 { b as usize } else { self.take(1)[0] as usize };
        (tag, self.take(len))
    }
}

/// Serve the database until the client goes quiet (2s without a
/// datagram). Handles GET, GETNEXT and GETBULK.
fn serve(sock: UdpSocket, db: HashMap<String, (u8, Vec<u8>)>) {
    sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut keys: Vec<String> = db.keys().cloned().collect();
    keys.sort_by_key(|k| arcs(k));
    loop {
        let mut buf = vec![0u8; 65535];
        let (n, from) = match sock.recv_from(&mut buf) {
            Ok(x) => x,
            Err(_) => break,
        };
        let mut r = R { b: &buf[..n], p: 0 };
        let (_, msg) = r.tlv();
        let mut m = R { b: msg, p: 0 };
        let _ = m.tlv();
        let _ = m.tlv();
        let (pdu_tag, pdu) = m.tlv();
        let mut p = R { b: pdu, p: 0 };
        let (_, id) = p.tlv();
        let (_, p1) = p.tlv();
        let (_, p2) = p.tlv();
        let (_, vbl) = p.tlv();
        let mut v = R { b: vbl, p: 0 };
        let mut want = Vec::new();
        while v.p < v.b.len() {
            let (_, vb) = v.tlv();
            let mut q = R { b: vb, p: 0 };
            let (_, name) = q.tlv();
            want.push(crate::core::snmp::proto::decode_oid(name).unwrap());
        }
        let max_rep = p2.iter().fold(0u64, |a, b| (a << 8) | *b as u64).max(1);
        let mut bindings: Vec<(String, u8, Vec<u8>)> = Vec::new();
        for oid in &want {
            match pdu_tag {
                0xa0 => match db.get(oid) {
                    Some((t, b)) => bindings.push((oid.clone(), *t, b.clone())),
                    None => bindings.push((oid.clone(), 0x80, vec![])),
                },
                0xa1 => match keys.iter().find(|k| arcs(k) > arcs(oid)) {
                    Some(k) => {
                        let (t, b) = &db[k];
                        bindings.push((k.clone(), *t, b.clone()));
                    }
                    None => bindings.push((oid.clone(), 0x82, vec![])),
                },
                _ => {
                    let mut cur = oid.clone();
                    for _ in 0..max_rep {
                        match keys.iter().find(|k| arcs(k) > arcs(&cur)) {
                            Some(k) => {
                                let (t, b) = &db[k];
                                bindings.push((k.clone(), *t, b.clone()));
                                cur = k.clone();
                            }
                            None => break,
                        }
                    }
                }
            }
        }
        let mut vbl_out = Vec::new();
        for (oid, tag, body) in &bindings {
            let mut vb = tlv(0x06, &enc_oid(oid));
            vb.extend_from_slice(&tlv(*tag, body));
            vbl_out.extend_from_slice(&tlv(0x30, &vb));
        }
        let mut pdu_out = Vec::new();
        pdu_out.extend_from_slice(&tlv(0x02, id));
        pdu_out.extend_from_slice(&tlv(0x02, p1));
        pdu_out.extend_from_slice(&tlv(0x02, p2));
        pdu_out.extend_from_slice(&tlv(0x30, &vbl_out));
        let mut msg_out = Vec::new();
        msg_out.extend_from_slice(&tlv(0x02, &[1]));
        msg_out.extend_from_slice(&tlv(0x04, b"public"));
        msg_out.extend_from_slice(&tlv(0xa2, &pdu_out));
        let resp = tlv(0x30, &msg_out);
        let _ = sock.send_to(&resp, from);
    }
}

fn num(snap: &crate::core::value::Value, plugin: &str, key: &str) -> f64 {
    snap.as_object()
        .and_then(|o| o.get(plugin))
        .and_then(|p| p.as_object())
        .and_then(|o| o.get(key))
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN)
}

#[test]
fn snmp_tick_fills_local_shapes() {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = sock.local_addr().unwrap().port();
    let agent = std::thread::spawn(move || serve(sock, db()));
    let mut client = SnmpClient::new("127.0.0.1", port, "2c", "public").unwrap();
    client.set_timeout(Duration::from_secs(5));
    let ctx = SnmpCtx::probe(&client).unwrap();
    assert_eq!(ctx.system_name.as_deref(), Some("linux"));
    let stats = GlancesStats::new(2.0);
    plugins::register_all(&stats);
    stats.update_snmp(&ctx).unwrap();
    agent.join().unwrap();
    let snap = stats.snapshot();
    assert_eq!(num(&snap, "cpu", "total"), 17.0); // 100 - idle(83)
    assert_eq!(num(&snap, "cpu", "user"), 12.0);
    assert_eq!(num(&snap, "mem", "total"), 100.0 * 1024.0);
    assert_eq!(num(&snap, "mem", "used"), 60.0 * 1024.0);
    assert_eq!(num(&snap, "load", "min1"), 0.08);
    assert_eq!(num(&snap, "uptime", "seconds"), 123_456.0);
    let host = snap
        .as_object().unwrap()["system"]
        .as_object().unwrap()["hostname"]
        .as_str()
        .unwrap();
    assert_eq!(host, "testbox");
}
