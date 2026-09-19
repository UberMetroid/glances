//! Minimal std-only JSON parser.
//!
//! Docker's /containers/json body and the cloud metadata responses
//! (AWS / GCP / Azure) all need to be parsed without pulling in
//! `serde_json`. This module provides a tiny recursive-descent parser
//! that handles exactly the surface those endpoints emit:
//!
//! - objects (`{ ... }`) → `BTreeMap<String, Value>`
//! - arrays  (`[ ... ]`) → `Vec<Value>`
//! - UTF-8 strings with the usual escapes (incl. `\uXXXX` surrogate
//!   pairs) — see `strparse.rs`
//! - integer and floating-point numbers (full JSON grammar, incl.
//!   exponents)
//! - `true` / `false` / `null`
//!
//! Anything unusual (e.g. `NaN`, comments, trailing garbage, malformed
//! escapes) returns `None` and the caller treats the input as "no
//! data". The parser never panics and nesting depth is capped so
//! pathological input cannot overflow the stack.

use std::collections::BTreeMap;

use crate::core::value::Value;

mod strparse;

/// Maximum nested object/array depth — deep input recurses on the Rust
/// stack; 128 is far beyond any Docker/cloud response.
const MAX_DEPTH: u32 = 128;

pub struct JsonParser<'a> {
    pub input: &'a [u8],
    pub pos: usize,
    depth: u32,
}

impl<'a> JsonParser<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0, depth: 0 }
    }

    pub fn skip_ws(&mut self) {
        while let Some(&b) = self.input.get(self.pos) {
            if matches!(b, b' ' | b'\n' | b'\r' | b'\t') { self.pos += 1; } else { break; }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    /// JSON number grammar:
    ///   `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`
    pub fn parse_number(&mut self) -> Option<Value> {
        fn digits(p: &mut JsonParser<'_>) -> bool {
            let mut any = false;
            while matches!(p.peek(), Some(b'0'..=b'9')) {
                p.pos += 1;
                any = true;
            }
            any
        }
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek()? {
            b'0' => self.pos += 1,
            b'1'..=b'9' => { digits(self); }
            _ => return None,
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            if !digits(self) {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            if !digits(self) {
                return None;
            }
        }
        let text = std::str::from_utf8(&self.input[start..self.pos]).ok()?;
        if is_float {
            text.parse::<f64>().ok().map(Value::Float)
        } else if text.starts_with('-') {
            text.parse::<i64>().ok().map(Value::Int)
        } else {
            text.parse::<u64>()
                .map(Value::Uint)
                .or_else(|_| text.parse::<i64>().map(Value::Int))
                .ok()
        }
    }

    pub fn parse_value(&mut self) -> Option<Value> {
        self.skip_ws();
        match self.peek()? {
            b'"' => self.parse_string().map(Value::String),
            b'{' => self.parse_object().map(Value::Object),
            b'[' => self.parse_array().map(Value::Array),
            b't' => if self.expect_lit(b"true") { Some(Value::Bool(true)) } else { None },
            b'f' => if self.expect_lit(b"false") { Some(Value::Bool(false)) } else { None },
            b'n' => if self.expect_lit(b"null") { Some(Value::Null) } else { None },
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    pub fn parse_array(&mut self) -> Option<Vec<Value>> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let r = self.parse_array_inner();
        self.depth -= 1;
        r
    }

    fn parse_array_inner(&mut self) -> Option<Vec<Value>> {
        if self.bump()? != b'[' {
            return None;
        }
        let mut out = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(out);
        }
        loop {
            out.push(self.parse_value()?);
            self.skip_ws();
            match self.bump()? {
                b',' => continue,
                b']' => return Some(out),
                _ => return None,
            }
        }
    }

    pub fn parse_object(&mut self) -> Option<BTreeMap<String, Value>> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let r = self.parse_object_inner();
        self.depth -= 1;
        r
    }

    fn parse_object_inner(&mut self) -> Option<BTreeMap<String, Value>> {
        if self.bump()? != b'{' {
            return None;
        }
        let mut out = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(out);
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.bump()? != b':' {
                return None;
            }
            let val = self.parse_value()?;
            out.insert(key, val);
            self.skip_ws();
            match self.bump()? {
                b',' => continue,
                b'}' => return Some(out),
                _ => return None,
            }
        }
    }
}

/// Parse a top-level JSON object from raw text. Trailing garbage after
/// the closing `}` is rejected.
pub fn parse_object(input: &str) -> Option<Value> {
    let mut p = JsonParser::new(input.as_bytes());
    p.skip_ws();
    let v = p.parse_object().map(Value::Object)?;
    p.skip_ws();
    if p.pos == input.len() { Some(v) } else { None }
}

/// Parse a top-level JSON array from raw text. Trailing garbage after
/// the closing `]` is rejected.
pub fn parse_array(input: &str) -> Option<Vec<Value>> {
    let mut p = JsonParser::new(input.as_bytes());
    p.skip_ws();
    let v = p.parse_array()?;
    p.skip_ws();
    if p.pos == input.len() { Some(v) } else { None }
}
