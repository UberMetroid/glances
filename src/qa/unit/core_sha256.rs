//! Unit tests for core::sha256 — FIPS 180-4 known answers.

use crate::core::sha256::sha256_hex;

#[test]
fn known_empty() {
    assert_eq!(sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}

#[test]
fn known_abc() {
    assert_eq!(sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn known_two_block() {
    assert_eq!(sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
}

#[test]
fn unicode_input_does_not_panic() {
    let h = sha256_hex("héllo 世界 🚀".as_bytes());
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}
