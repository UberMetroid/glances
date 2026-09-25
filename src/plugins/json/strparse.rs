//! String + escape decoding for the JSON parser (split out for the
//! file-size lint). Implements `JsonParser::parse_string` and helpers.

use super::JsonParser;

impl JsonParser<'_> {
    /// Decode the UTF-8 scalar starting at `pos`. Multi-byte sequences
    /// are validated (rejects overlongs, surrogates, truncation).
    fn bump_char(&mut self) -> Option<char> {
        let b = self.peek()?;
        if b < 0x80 {
            self.pos += 1;
            return Some(b as char);
        }
        let width = match b {
            0xF0..=0xF4 => 4,
            0xE0..=0xEF => 3,
            0xC2..=0xDF => 2,
            _ => return None,
        };
        let s = std::str::from_utf8(self.input.get(self.pos..self.pos + width)?).ok()?;
        let c = s.chars().next()?;
        self.pos += width;
        Some(c)
    }

    /// Consume `s` exactly; the byte after must not continue the literal
    /// (rejects `truex`, `null0`, ...).
    pub(super) fn expect_lit(&mut self, s: &[u8]) -> bool {
        if self.input.get(self.pos..self.pos + s.len()) != Some(s) {
            return false;
        }
        if let Some(&b) = self.input.get(self.pos + s.len())
            && (b.is_ascii_alphanumeric() || b == b'_') {
                return false;
            }
        self.pos += s.len();
        true
    }

    /// Read 4 hex digits after `\u`.
    fn hex4(&mut self) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            v = (v << 4) | (self.bump()? as char).to_digit(16)?;
        }
        Some(v)
    }

    /// Decode one `\uXXXX` escape, joining surrogate pairs.
    fn unicode_escape(&mut self) -> Option<char> {
        let cp = self.hex4()?;
        if (0xD800..0xDC00).contains(&cp) {
            // High surrogate — must be followed by \uDC00..\uDFFF.
            if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                return None;
            }
            let lo = self.hex4()?;
            if !(0xDC00..0xE000).contains(&lo) {
                return None;
            }
            char::from_u32(0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00))
        } else if (0xDC00..0xE000).contains(&cp) {
            None // lone low surrogate
        } else {
            char::from_u32(cp)
        }
    }

    pub fn parse_string(&mut self) -> Option<String> {
        if self.bump()? != b'"' {
            return None;
        }
        let mut s = String::new();
        loop {
            match self.peek()? {
                b'"' => {
                    self.pos += 1;
                    return Some(s);
                }
                b'\\' => {
                    self.pos += 1;
                    match self.bump()? {
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        b'/' => s.push('/'),
                        b'b' => s.push('\u{8}'),
                        b'f' => s.push('\u{c}'),
                        b'n' => s.push('\n'),
                        b'r' => s.push('\r'),
                        b't' => s.push('\t'),
                        b'u' => s.push(self.unicode_escape()?),
                        _ => return None,
                    }
                }
                _ => {
                    let c = self.bump_char()?;
                    // JSON forbids unescaped control characters < U+0020.
                    if (c as u32) < 0x20 {
                        return None;
                    }
                    s.push(c);
                }
            }
        }
    }
}
