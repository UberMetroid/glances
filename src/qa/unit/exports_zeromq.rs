//! Unit tests for the ZeroMQ exporter (framing + live loopback handshake).

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use crate::core::value::Value;
use crate::exports::flatten::collect;
use crate::exports::zeromq;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

#[test]
fn greeting_is_valid_and_64_bytes() {
    let g = zeromq::greeting();
    assert_eq!(g.len(), 64);
    assert!(zeromq::valid_greeting(&g));
    assert_eq!(g[10], 3, "ZMTP major version");
    assert_eq!(g[32], 1, "as-server flag");
}

#[test]
fn greeting_rejects_garbage() {
    assert!(!zeromq::valid_greeting(&[0u8; 64]));
    assert!(!zeromq::valid_greeting(&[0u8; 10]));
}

#[test]
fn encode_frame_short_and_long_forms() {
    let short = zeromq::encode_frame(b"ab", true);
    assert_eq!(short, vec![0x01, 2, b'a', b'b']);
    let last = zeromq::encode_frame(b"ab", false);
    assert_eq!(last[0], 0x00);
    let big = vec![9u8; 300];
    let long = zeromq::encode_frame(&big, false);
    assert_eq!(long[0], 0x02);
    assert_eq!(&long[1..9], &300u64.to_be_bytes());
    assert_eq!(long.len(), 9 + 300);
}

#[test]
fn ready_command_carries_socket_type() {
    let cmd = zeromq::ready_command("PUB");
    assert_eq!(cmd[0], 0x04);
    let body = &cmd[2..];
    assert!(body.windows(5).any(|w| w == b"READY"));
    assert!(body.windows(11).any(|w| w == b"Socket-Type"));
    assert!(body.windows(3).any(|w| w == b"PUB"));
}

#[test]
fn group_payloads_joins_elem_keys() {
    let mut keys = HashMap::new();
    keys.insert("fs".to_string(), "mntpoint");
    let snap = obj(&[(
        "fs",
        Value::Array(vec![obj(&[
            ("mntpoint", Value::String("/".into())),
            ("percent", Value::Float(10.0)),
        ])]),
    )]);
    let payloads = zeromq::group_payloads(&collect(&snap, &keys));
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0].0, "fs");
    assert!(payloads[0].1.contains("/.percent") || payloads[0].1.contains("percent"));
}

/// Read one ZMTP frame from the stream.
fn read_frame(s: &mut TcpStream) -> Vec<u8> {
    let mut flag = [0u8; 1];
    s.read_exact(&mut flag).unwrap();
    assert_eq!(flag[0] & 0x04, 0, "must be a message, not a command");
    let size = if flag[0] & 0x02 == 0 {
        let mut n = [0u8; 1];
        s.read_exact(&mut n).unwrap();
        n[0] as usize
    } else {
        let mut n = [0u8; 8];
        s.read_exact(&mut n).unwrap();
        u64::from_be_bytes(n) as usize
    };
    let mut buf = vec![0u8; size];
    s.read_exact(&mut buf).unwrap();
    buf
}

#[test]
fn loopback_handshake_and_broadcast() {
    // Bind a publisher on an ephemeral port and drive a raw SUB peer.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (conn, _) = listener.accept().expect("accept");
        let mut pub_ = zeromq::Publisher::new();
        let ready = zeromq::handshake(conn, Duration::from_secs(5)).expect("handshake");
        pub_.peers.push(ready);
        pub_.broadcast("glances", "cpu", br#"{"total":1}"#);
    });
    let mut peer = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    // Client greeting (not a server) + READY as SUB.
    let mut g = zeromq::greeting();
    g[32] = 0;
    peer.write_all(&g).unwrap();
    let mut sg = [0u8; 64];
    peer.read_exact(&mut sg).unwrap();
    assert!(zeromq::valid_greeting(&sg));
    peer.write_all(&zeromq::ready_command("SUB")).unwrap();
    // Server READY command.
    let mut flag = [0u8; 1];
    peer.read_exact(&mut flag).unwrap();
    assert_eq!(flag[0], 0x04);
    let mut n = [0u8; 1];
    peer.read_exact(&mut n).unwrap();
    let mut cmd = vec![0u8; n[0] as usize];
    peer.read_exact(&mut cmd).unwrap();
    // Three broadcast frames: prefix, plugin, payload.
    assert_eq!(read_frame(&mut peer).as_slice(), b"glances");
    assert_eq!(read_frame(&mut peer).as_slice(), b"cpu");
    assert_eq!(read_frame(&mut peer).as_slice(), br#"{"total":1}"#);
    server.join().expect("server");
}
