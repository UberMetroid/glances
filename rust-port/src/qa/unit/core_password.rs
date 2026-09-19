use crate::core::password::{sha256_hex, PasswordFile};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn sha256_known_abc() {
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn sha256_known_empty() {
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}

#[test]
fn password_file_format_roundtrip() {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!("glances-rs-pwd-{nanos}"));
    let mut pf = PasswordFile::empty();
    pf.path = path.clone();
    pf.entries.insert("alice".into(), sha256_hex(b"hunter2"));
    pf.save().unwrap();
    let loaded = PasswordFile::load(&path).unwrap();
    assert!(loaded.check("alice", "hunter2"));
    assert!(!loaded.check("alice", "wrong"));
    assert!(!loaded.check("bob", "hunter2"));
    let _ = fs::remove_file(path);
}

#[test]
fn password_file_accepts_python_format() {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!("glances-rs-py-{nanos}"));
    let hash = sha256_hex(b"secret");
    let content = format!("# glances password file\nadmin:{hash}\nbob:deadbeefcafebabe\n");
    fs::write(&path, content).unwrap();
    let loaded = PasswordFile::load(&path).unwrap();
    assert!(loaded.check("admin", "secret"));
    assert!(!loaded.check("admin", "other"));
    assert!(!loaded.check("bob", "secret"));
    let _ = fs::remove_file(path);
}

#[test]
fn password_file_missing_yields_empty() {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!("glances-rs-missing-{nanos}"));
    let pf = PasswordFile::load(&path).unwrap();
    assert!(!pf.check("anyone", "anything"));
}
