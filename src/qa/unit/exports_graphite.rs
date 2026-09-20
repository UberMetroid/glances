//! Unit tests for the Graphite plaintext exporter.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::net::TcpListener;
use std::time::Duration;

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::graphite;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
}

fn cfg() -> graphite::Config {
    graphite::Config::default()
}

#[test]
fn render_line_formats_metric_value_ts() {
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(42.0))]))]);
    let fields = flat(&snap);
    let line = graphite::render_line("glances", &fields[0], 42.0, 1_700_000_000).unwrap();
    assert_eq!(line, "glances.cpu.total 42 1700000000\n");
}

#[test]
fn render_line_lowercases_and_sanitizes() {
    let snap = obj(&[("CPU 0", obj(&[("RX Bytes", Value::Float(1.0))]))]);
    let fields = flat(&snap);
    let line = graphite::render_line("Glances DC", &fields[0], 1.0, 100).unwrap();
    assert!(line.starts_with("glances_dc.cpu_0.rx_bytes 1 100\n"), "got: {}", line);
}

#[test]
fn render_line_skips_nan_and_inf() {
    let snap = obj(&[("cpu", obj(&[("x", Value::Float(1.0))]))]);
    let fields = flat(&snap);
    assert!(graphite::render_line("g", &fields[0], f64::NAN, 0).is_none());
    assert!(graphite::render_line("g", &fields[0], f64::INFINITY, 0).is_none());
}

#[test]
fn build_body_skips_non_numeric() {
    let snap = obj(&[(
        "cpu",
        obj(&[
            ("good", Value::Int(1)),
            ("bad", Value::Float(f64::NAN)),
            ("str", Value::String("hi".into())),
        ]),
    )]);
    let body = graphite::build_body(&flat(&snap), &cfg(), 100);
    assert!(body.contains("glances.cpu.good 1 100\n"));
    assert!(!body.contains("bad"));
    assert!(!body.contains("str"));
}

#[test]
fn write_sends_plaintext_to_carbon() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).expect("read");
        String::from_utf8(buf).expect("utf8")
    });
    let snap = obj(&[("cpu", obj(&[("total", Value::Float(12.5))]))]);
    let mut c = cfg();
    c.host = "127.0.0.1".into();
    c.port = port;
    graphite::write(&flat(&snap), &c).expect("write");
    let body = handle.join().expect("thread");
    assert!(body.contains("glances.cpu.total 12.5 "), "got: {}", body);
}
