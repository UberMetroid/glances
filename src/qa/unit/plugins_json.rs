//! Tests for the std-only JSON parser (plugins::json).
//!
//! Regression coverage for: UTF-8 corruption (`c as char` on raw bytes),
//! missing exponent support, unchecked literals, trailing garbage, and
//! unbounded recursion.

use crate::core::value::Value;
use crate::plugins::json::{parse_array, parse_object};

#[test]
fn utf8_strings_round_trip() {
    // "café" as raw UTF-8 bytes — the old parser emitted "cafÃ©".
    let v = parse_object("{\"name\":\"café\"}").unwrap();
    let m = v.as_object().unwrap();
    assert_eq!(m.get("name").and_then(Value::as_str), Some("café"));
    let v = parse_object("{\"n\":\"日本語\"}").unwrap();
    assert_eq!(v.as_object().unwrap().get("n").and_then(Value::as_str), Some("日本語"));
}

#[test]
fn unicode_escapes_and_surrogate_pairs() {
    let v = parse_object(r#"{"a":"é","b":"𝄞"}"#).unwrap();
    let m = v.as_object().unwrap();
    assert_eq!(m.get("a").and_then(Value::as_str), Some("é"));
    assert_eq!(m.get("b").and_then(Value::as_str), Some("𝄞"));
}

#[test]
fn numbers_follow_full_json_grammar() {
    let v = parse_array("[1e3,-2.5e-2,0,1.5,1E+2,-0]").unwrap();
    let f = |i: usize| v[i].as_f64().unwrap();
    assert_eq!(f(0), 1000.0);
    assert_eq!(f(1), -0.025);
    assert_eq!(f(2), 0.0);
    assert_eq!(f(3), 1.5);
    assert_eq!(f(4), 100.0);
    assert_eq!(f(5), -0.0);
}

#[test]
fn malformed_numbers_rejected() {
    assert!(parse_array("[1.]").is_none());   // fraction needs digits
    assert!(parse_array("[.5]").is_none());   // no leading digit
    assert!(parse_array("[01]").is_none());   // leading zero
    assert!(parse_array("[1e]").is_none());   // exponent needs digits
    assert!(parse_array("[--1]").is_none());
    assert!(parse_array("[1x]").is_none());
}

#[test]
fn literals_must_be_exact() {
    let v = parse_array("[true,false,null]").unwrap();
    assert_eq!(v[0], Value::Bool(true));
    assert_eq!(v[1], Value::Bool(false));
    assert_eq!(v[2], Value::Null);
    assert!(parse_array("[truex]").is_none());
    assert!(parse_array("[nul]").is_none());
    assert!(parse_array("[falsey]").is_none());
    assert!(parse_array("[NaN]").is_none());
    assert!(parse_array("[Infinity]").is_none());
}

#[test]
fn trailing_garbage_rejected() {
    assert!(parse_object("{\"a\":1} x").is_none());
    assert!(parse_object("{\"a\":1}{\"b\":2}").is_none());
    assert!(parse_array("[1] extra").is_none());
    // Whitespace after the close is fine.
    assert!(parse_object("{\"a\":1} \n\t").is_some());
}

#[test]
fn malformed_strings_rejected() {
    assert!(parse_object("{\"a\":\"unterminated}").is_none());
    assert!(parse_object("{\"a\":\"bad\\q\"}").is_none()); // unknown escape
    assert!(parse_object("{\"a\":\"\\uZZZZ\"}").is_none());
    // Lone low surrogate.
    assert!(parse_object("{\"a\":\"\\uDC00\"}").is_none());
    // Unescaped control character in a string.
    assert!(parse_object("{\"a\":\"x\ny\"}").is_none());
}

#[test]
fn nesting_depth_is_capped() {
    let deep = "[".repeat(200) + &"]".repeat(200);
    assert!(parse_array(&deep).is_none(), "200-deep input must fail, not crash");
    let ok = "[".repeat(100) + "1" + &"]".repeat(100);
    assert!(parse_array(&ok).is_some());
}

#[test]
fn truncated_structures_rejected() {
    assert!(parse_object("{\"a\":").is_none());
    assert!(parse_object("{\"a\"").is_none());
    assert!(parse_array("[1,").is_none());
    assert!(parse_object("{\"a\":1,").is_none());
}
