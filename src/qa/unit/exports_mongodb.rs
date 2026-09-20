//! Unit tests for the MongoDB OP_MSG exporter.

use std::collections::{BTreeMap, HashMap};

use crate::core::value::Value;
use crate::exports::flatten::{collect, Field};
use crate::exports::mongodb;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

fn flat(snap: &Value) -> Vec<Field<'_>> {
    collect(snap, &HashMap::new())
}

#[test]
fn bson_document_is_length_prefixed_and_zero_terminated() {
    let doc = mongodb::bson_document(&[("x", Value::Int(1))]);
    let declared = i32::from_le_bytes([doc[0], doc[1], doc[2], doc[3]]);
    assert_eq!(declared as usize, doc.len());
    assert_eq!(doc[doc.len() - 1], 0u8);
}

#[test]
fn bson_ints_encode_as_int64() {
    // Ints are always BSON int64 (0x12) — a >2GiB counter must not
    // wrap negative via int32.
    let doc = mongodb::bson_document(&[("big", Value::Int(5_000_000_000))]);
    let needle = [0x12, b'b', b'i', b'g', 0];
    let i = doc.windows(5).position(|w| w == needle).expect("int64 elem");
    let v = i64::from_le_bytes(doc[i + 5..i + 13].try_into().unwrap());
    assert_eq!(v, 5_000_000_000);
}

#[test]
fn bson_string_field_carries_length_plus_nul_terminator() {
    let doc = mongodb::bson_document(&[("k", Value::String("hi".into()))]);
    // Element header = 0x02 (string) + "k\0" + i32 str_len (incl. NUL) + bytes + 0x00
    let needle: Vec<u8> = vec![0x02, b'k', 0, 3, 0, 0, 0, b'h', b'i', 0];
    assert!(doc.windows(needle.len()).any(|w| w == needle),
        "expected needle {:?} in doc {:?}", needle, doc);
}

#[test]
fn op_msg_frame_has_correct_opcode_and_length() {
    let bson = mongodb::bson_document(&[("insert", Value::String("stats".into()))]);
    let msg = mongodb::build_op_msg(&bson);
    let declared = i32::from_le_bytes([msg[0], msg[1], msg[2], msg[3]]);
    let opcode = i32::from_le_bytes([msg[12], msg[13], msg[14], msg[15]]);
    assert_eq!(declared as usize, msg.len());
    assert_eq!(opcode, 2013); // OP_MSG
}

#[test]
fn body_is_insert_command_with_documents_and_db() {
    // The OP_MSG body must be a real `insert` command:
    // {insert: <coll>, documents: [{<series>: {...}}], $db: <db>}.
    let cfg = mongodb::Config::default();
    let snap = obj(&[("cpu", obj(&[("total", Value::Int(1))]))]);
    let body = mongodb::build_body(&flat(&snap), &cfg);
    let raw = String::from_utf8_lossy(&body);
    assert!(raw.contains("insert"), "missing insert command: {:?}", raw);
    assert!(raw.contains("stats"), "missing collection name");
    assert!(raw.contains("documents"), "missing documents array");
    assert!(raw.contains("$db"), "missing $db field");
    assert!(raw.contains("glances"), "missing database name");
}

#[test]
fn nan_floats_render_as_bson_null_element() {
    let doc = mongodb::bson_document(&[("v", Value::Float(f64::NAN))]);
    // BSON type 0x0A = null. Element bytes start with [0x0A, b'v', 0].
    assert!(doc.windows(3).any(|w| w == [0x0A, b'v', 0]),
        "doc was: {:?}", doc);
}

#[test]
fn empty_database_or_collection_rejected() {
    let snap = obj(&[("cpu", obj(&[("x", Value::Int(1))]))]);
    let cfg = mongodb::Config { database: String::new(), ..Default::default() };
    let err = mongodb::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
    let cfg = mongodb::Config { collection: String::new(), ..Default::default() };
    let err = mongodb::write(&flat(&snap), &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}
