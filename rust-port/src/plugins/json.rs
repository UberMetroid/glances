//! Minimal std-only JSON parser.
//!
//! Docker's /containers/json body and the cloud metadata responses
//! (AWS / GCP / Azure) all need to be parsed without pulling in
//! `serde_json`. This module provides a tiny recursive-descent parser
//! that handles exactly the surface those endpoints emit:
//!
//! - objects (`{ ... }`) → `BTreeMap<String, Value>`
//! - arrays  (`[ ... ]`) → `Vec<Value>`
//! - strings with the usual escape sequences
//! - integer and floating-point numbers
//! - `true` / `false` / `null`
//!
//! Anything unusual (e.g. `NaN`, comments, leading whitespace inside
//! tokens) returns `None` and the caller treats the input as "no
//! data". The parser never panics.

use std::collections::BTreeMap;

use crate::core::value::Value;

pub struct JsonParser<'a> {
    pub input: &'a [u8],
    pub pos: usize,
}

impl<'a> JsonParser<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    pub fn skip_ws(&mut self) {
        while let Some(&b) = self.input.get(self.pos) {
            if matches!(b, b' ' | b'\n' | b'\r' | b'\t') {
                self.pos += 1;
            } else {
                break;
            }
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

    fn skip_lit(&mut self, s: &[u8]) {
        for b in s {
            if self.peek() == Some(*b) {
                self.pos += 1;
            }
        }
    }

    pub fn parse_string(&mut self) -> Option<String> {
        if self.bump()? != b'"' {
            return None;
        }
        let mut s = String::new();
        loop {
            match self.bump()? {
                b'"' => return Some(s),
                b'\\' => match self.bump()? {
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    b'/' => s.push('/'),
                    b'n' => s.push('\n'),
                    b'r' => s.push('\r'),
                    b't' => s.push('\t'),
                    b'u' => {
                        let mut hex = [0u8; 4];
                        for h in &mut hex {
                            *h = self.bump()?;
                        }
                        if let Ok(cp) = u32::from_str_radix(
                            std::str::from_utf8(&hex).ok()?,
                            16,
                        ) {
                            if let Some(c) = char::from_u32(cp) {
                                s.push(c);
                            }
                        }
                    }
                    _ => return None,
                },
                c => s.push(c as char),
            }
        }
    }

    pub fn parse_number(&mut self) -> Option<Value> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let mut has_dot = false;
        while let Some(b) = self.peek() {
            match b {
                b'0'..=b'9' => self.pos += 1,
                b'.' if !has_dot => {
                    has_dot = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        let text = std::str::from_utf8(&self.input[start..self.pos]).ok()?;
        if has_dot {
            text.parse::<f64>().ok().map(Value::Float)
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
            b't' => {
                self.skip_lit(b"true");
                Some(Value::Bool(true))
            }
            b'f' => {
                self.skip_lit(b"false");
                Some(Value::Bool(false))
            }
            b'n' => {
                self.skip_lit(b"null");
                Some(Value::Null)
            }
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    pub fn parse_array(&mut self) -> Option<Vec<Value>> {
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

/// Parse a top-level JSON object from raw text.
pub fn parse_object(input: &str) -> Option<Value> {
    let mut p = JsonParser::new(input.as_bytes());
    p.skip_ws();
    p.parse_object().map(Value::Object)
}

/// Parse a top-level JSON array from raw text.
pub fn parse_array(input: &str) -> Option<Vec<Value>> {
    let mut p = JsonParser::new(input.as_bytes());
    p.skip_ws();
    p.parse_array()
}
