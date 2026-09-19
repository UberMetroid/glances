//! Unit tests for core::hex.

use crate::core::hex::{const_time_eq, decode, encode};

#[test]
fn encode_basic() {
    assert_eq!(encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    assert_eq!(encode(&[]), "");
    assert_eq!(encode(&[0xff]), "ff");
}

#[test]
fn decode_basic() {
    assert_eq!(decode("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    assert_eq!(decode("DEADBEEF"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    assert_eq!(decode(""), Some(vec![]));
}

#[test]
fn decode_rejects_bad_input() {
    assert_eq!(decode("xyz"), None);
    assert_eq!(decode("abc"), None); // odd length
    assert_eq!(decode("abcz"), None); // bad nibble
}

#[test]
fn roundtrip_random() {
    for n in 0..100 {
        let bytes: Vec<u8> = (0..n).map(|i| (i * 7 + 13) as u8).collect();
        assert_eq!(decode(&encode(&bytes)), Some(bytes));
    }
}

#[test]
fn const_time_eq_basics() {
    assert!(const_time_eq(b"hello", b"hello"));
    assert!(!const_time_eq(b"hello", b"world"));
    assert!(!const_time_eq(b"hello", b"hell"));
    assert!(!const_time_eq(b"", b"x"));
    assert!(const_time_eq(b"", b""));
}
