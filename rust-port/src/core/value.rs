//! Value — our JSON-compatible value type (no `serde_json`).
//!
//! This replaces serde_json::Value so we can stay std-only (AC-1). It is
//! the data contract every plugin uses to expose its stats.

use std::collections::BTreeMap;

/// JSON-compatible value. Plugins return `Value::Object` for dict-style
/// stats or `Value::Array` for list-of-dict stats (e.g. process list).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }
    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        if let Value::Object(m) = self { Some(m) } else { None }
    }
    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        if let Value::Object(m) = self { Some(m) } else { None }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Value::String(s) = self { Some(s.as_str()) } else { None }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Uint(u) => Some(*u as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Uint(u) => i64::try_from(*u).ok(),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self { Value::Bool(v) }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self { Value::Int(v) }
}
impl From<u64> for Value {
    fn from(v: u64) -> Self { Value::Uint(v) }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self { Value::Float(v) }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self { Value::String(v.to_string()) }
}
impl From<String> for Value {
    fn from(v: String) -> Self { Value::String(v) }
}
impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self { Value::Array(v) }
}
impl From<BTreeMap<String, Value>> for Value {
    fn from(v: BTreeMap<String, Value>) -> Self { Value::Object(v) }
}

/// Serialize a `Value` to a JSON string. Mirrors Python Glances'
/// `glances.globals.json_dumps`: floats formatted without NaN/Infinity
/// (which would break JSON parsers downstream).
pub fn to_json(v: &Value) -> String {
    let mut buf = String::new();
    write_json(v, &mut buf, 0);
    buf
}

fn write_json(v: &Value, buf: &mut String, indent: usize) {
    match v {
        Value::Null => buf.push_str("null"),
        Value::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            buf.push_str(&i.to_string());
        }
        Value::Uint(u) => {
            buf.push_str(&u.to_string());
        }
        Value::Float(f) => {
            if f.is_nan() || f.is_infinite() {
                // Match Python's json_dumps: skip NaN/Infinity (renders as null).
                buf.push_str("null");
            } else {
                buf.push_str(&format!("{:.6}", f));
            }
        }
        Value::String(s) => {
            buf.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => buf.push_str("\\\""),
                    '\\' => buf.push_str("\\\\"),
                    '\n' => buf.push_str("\\n"),
                    '\r' => buf.push_str("\\r"),
                    '\t' => buf.push_str("\\t"),
                    c if (c as u32) < 0x20 => {
                        buf.push_str(&format!("\\u{:04x}", c as u32));
                    }
                    c => buf.push(c),
                }
            }
            buf.push('"');
        }
        Value::Array(arr) => {
            buf.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 { buf.push(','); }
                write_json(item, buf, indent);
            }
            buf.push(']');
        }
        Value::Object(obj) => {
            buf.push('{');
            for (i, (k, val)) in obj.iter().enumerate() {
                if i > 0 { buf.push(','); }
                buf.push('"');
                buf.push_str(&escape_key(k));
                buf.push_str("\":");
                write_json(val, buf, indent);
            }
            buf.push('}');
        }
    }
}

fn escape_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out
}
