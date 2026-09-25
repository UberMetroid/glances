//! Minimal BER codec for SNMP (v1 community / v2c community).
//!
//! Supports exactly what the client needs: INTEGER, OCTET STRING,
//! NULL, OBJECT IDENTIFIER, IpAddress, Counter32, Gauge32, TimeTicks,
//! Counter64, and SNMP exception markers (noSuchObject 0x80,
//! noSuchInstance 0x81, endOfMibView 0x82). No crafted-input
//! parsing beyond strict bounds checks — every length is validated.

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self { Self { buf, pos: 0 } }

    pub fn is_empty(&self) -> bool { self.pos >= self.buf.len() }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.buf.len() {
            return Err("BER: truncated input".into());
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn tag(&mut self) -> Result<u8, String> {
        Ok(*self.take(1)?.first().unwrap_or(&0))
    }

    pub fn len(&mut self) -> Result<usize, String> {
        let b = *self.take(1)?.first().unwrap_or(&0);
        if b & 0x80 == 0 {
            return Ok(b as usize);
        }
        let n = (b & 0x7f) as usize;
        if n == 0 || n > 4 {
            return Err("BER: bad length".into());
        }
        let mut v = 0usize;
        for b in self.take(n)? {
            v = (v << 8) | *b as usize;
        }
        Ok(v)
    }

    /// Read one TLV, returning (tag, contents).
    pub fn tlv(&mut self) -> Result<(u8, &'a [u8]), String> {
        let tag = self.tag()?;
        let len = self.len()?;
        Ok((tag, self.take(len)?))
    }

    /// Expect a SEQUENCE (0x30), returning a sub-reader.
    pub fn seq(&mut self) -> Result<Reader<'a>, String> {
        let (tag, body) = self.tlv()?;
        if tag != 0x30 {
            return Err(format!("BER: expected SEQUENCE, got {:#x}", tag));
        }
        Ok(Reader::new(body))
    }
}

fn emit_len(out: &mut Vec<u8>, n: usize) {
    if n < 128 {
        out.push(n as u8);
    } else {
        let mut tmp = [0u8; 8];
        let mut len = 0usize;
        let mut v = n;
        while v > 0 {
            len += 1;
            tmp[8 - len] = (v & 0xff) as u8;
            v >>= 8;
        }
        out.push(0x80 | len as u8);
        out.extend_from_slice(&tmp[8 - len..]);
    }
}

fn emit(out: &mut Vec<u8>, tag: u8, body: &[u8]) {
    out.push(tag);
    emit_len(out, body.len());
    out.extend_from_slice(body);
}

fn emit_int_body(out: &mut Vec<u8>, mut v: i64) {
    let mut bytes = [0u8; 8];
    for i in (0..8).rev() {
        bytes[i] = (v & 0xff) as u8;
        v >>= 8;
    }
    // Minimal two's-complement form.
    let mut start = 0;
    while start < 7 {
        let redundant = if bytes[start] == 0x00 {
            bytes[start + 1] & 0x80 == 0
        } else if bytes[start] == 0xff {
            bytes[start + 1] & 0x80 != 0
        } else {
            false
        };
        if redundant { start += 1; } else { break; }
    }
    out.extend_from_slice(&bytes[start..]);
}

pub fn emit_int(out: &mut Vec<u8>, v: i64) {
    let mut body = Vec::new();
    emit_int_body(&mut body, v);
    emit(out, 0x02, &body);
}

/// Encode a dotted OID into contents bytes.
pub fn encode_oid(oid: &str) -> Result<Vec<u8>, String> {
    let parts: Vec<u64> = oid
        .split('.')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u64>().map_err(|_| format!("bad OID {}", oid)))
        .collect::<Result<_, _>>()?;
    if parts.len() < 2 {
        return Err(format!("bad OID {}", oid));
    }
    let mut out = vec![(parts[0] * 40 + parts[1]) as u8];
    for p in &parts[2..] {
        let mut stack = [0u8; 10];
        let mut n = 0;
        let mut v = *p;
        stack[n] = (v & 0x7f) as u8;
        n += 1;
        v >>= 7;
        while v > 0 {
            stack[n] = 0x80 | (v & 0x7f) as u8;
            n += 1;
            v >>= 7;
        }
        for b in stack[..n].iter().rev() {
            out.push(*b);
        }
    }
    Ok(out)
}

/// Decode OID contents bytes to dotted form.
pub fn decode_oid(body: &[u8]) -> Result<String, String> {
    if body.is_empty() {
        return Err("BER: empty OID".into());
    }
    let mut parts = vec![(body[0] / 40).to_string(), (body[0] % 40).to_string()];
    let mut v: u64 = 0;
    for b in &body[1..] {
        v = (v << 7) | (*b & 0x7f) as u64;
        if b & 0x80 == 0 {
            parts.push(v.to_string());
            v = 0;
        }
    }
    Ok(parts.join("."))
}

fn decode_int(body: &[u8]) -> Result<i64, String> {
    if body.is_empty() || body.len() > 8 {
        return Err("BER: bad INTEGER".into());
    }
    let mut v: i64 = if body[0] & 0x80 != 0 { -1 } else { 0 };
    for b in body {
        v = (v << 8) | *b as i64;
    }
    Ok(v)
}

fn decode_uint(body: &[u8]) -> Result<u64, String> {
    if body.len() > 8 {
        return Err("BER: bad unsigned".into());
    }
    let mut v: u64 = 0;
    for b in body {
        v = (v << 8) | *b as u64;
    }
    Ok(v)
}

/// Decode a VarBind value by tag.
pub fn decode_value(tag: u8, body: &[u8]) -> Result<super::SnmpValue, String> {
    use super::SnmpValue;
    match tag {
        0x02 => Ok(SnmpValue::Int(decode_int(body)?)),
        0x04 => Ok(SnmpValue::Str(String::from_utf8_lossy(body).into_owned())),
        0x05 => Ok(SnmpValue::Null),
        0x06 => Ok(SnmpValue::Str(decode_oid(body)?)),
        0x40 if body.len() == 4 => {
            Ok(SnmpValue::Ip([body[0], body[1], body[2], body[3]]))
        }
        0x41 | 0x42 | 0x43 | 0x46 => Ok(SnmpValue::Uint(decode_uint(body)?)),
        0x80..=0x82 => Ok(SnmpValue::Exception(tag)),
        _ => Err(format!("BER: unsupported value tag {:#x}", tag)),
    }
}

/// Build a full SNMP message. `pdu_tag`: 0xa0 GET, 0xa5 GETBULK.
/// `bulk_max` is max-repetitions for GETBULK (ignored otherwise;
/// callers pass 0 for GET so error-status/error-index stay zero).
pub fn build_message(
    version: i64,
    community: &str,
    pdu_tag: u8,
    request_id: i64,
    bulk_max: i64,
    oid_bodies: &[Vec<u8>],
) -> Vec<u8> {
    let mut vb_list = Vec::new();
    for body in oid_bodies {
        let mut vb = Vec::new();
        emit(&mut vb, 0x06, body);
        emit(&mut vb, 0x05, &[]);
        let mut seq = Vec::new();
        emit(&mut seq, 0x30, &vb);
        vb_list.extend_from_slice(&seq);
    }
    let mut pdu_body = Vec::new();
    emit_int(&mut pdu_body, request_id);
    emit_int(&mut pdu_body, 0); // error-status / non-repeaters
    emit_int(&mut pdu_body, bulk_max); // error-index / max-repetitions
    let mut vl = Vec::new();
    emit(&mut vl, 0x30, &vb_list);
    pdu_body.extend_from_slice(&vl);
    let mut pdu = Vec::new();
    emit(&mut pdu, pdu_tag, &pdu_body);
    let mut msg_body = Vec::new();
    emit_int(&mut msg_body, version);
    emit(&mut msg_body, 0x04, community.as_bytes());
    msg_body.extend_from_slice(&pdu);
    let mut msg = Vec::new();
    emit(&mut msg, 0x30, &msg_body);
    msg
}
