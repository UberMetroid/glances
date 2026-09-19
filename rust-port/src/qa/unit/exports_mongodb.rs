//! Unit tests for the MongoDB OP_MSG exporter.

use std::collections::BTreeMap;

use crate::core::value::Value;
use crate::exports::mongodb;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs { m.insert((*k).to_string(), v.clone()); }
    Value::Object(m)
}

#[test]
fn bson_document_is_length_prefixed_and_zero_terminated() {
    let doc = mongodb::bson_document(&[("x", Value::Int(1))]);
    let declared = i32::from_le_bytes([doc[0], doc[1], doc[2], doc[3]]);
    assert_eq!(declared as usize, doc.len());
    assert_eq!(doc[doc.len() - 1], 0u8);
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
    let err = mongodb::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
    let cfg = mongodb::Config { collection: String::new(), ..Default::default() };
    let err = mongodb::write(&snap, &cfg).unwrap_err();
    assert!(matches!(err, crate::core::error::GlancesError::InvalidConfig(_)));
}