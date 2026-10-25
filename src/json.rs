//! Strict JSON parsing and canonical serialization (spec §3.4).

use crate::decimal::Decimal;
use crate::value::{Record, Value};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonError {
    /// Ordinary `json` error with reason and UTF-8 byte offset.
    Invalid { reason: &'static str, offset: usize },
    /// A numeric exponent beyond host range (resource abort).
    Overflow,
    /// Stopped at an interrupt checkpoint (see `crate::interrupt`).
    Interrupted,
}

struct P<'a> {
    s: &'a [u8],
    src: &'a str,
    i: usize,
}

fn invalid(reason: &'static str, offset: usize) -> JsonError {
    JsonError::Invalid { reason, offset }
}

/// Parse exactly one JSON value with optional surrounding whitespace.
pub fn parse(src: &str) -> Result<Value, JsonError> {
    if src.starts_with('\u{FEFF}') {
        return Err(invalid("bom", 0));
    }
    let mut p = P { s: src.as_bytes(), src, i: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(invalid("trailing", p.i));
    }
    Ok(v)
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn lit(&mut self, word: &str, v: Value) -> Result<Value, JsonError> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(invalid("syntax", self.i))
        }
    }

    fn value(&mut self) -> Result<Value, JsonError> {
        if crate::interrupt::poll() {
            return Err(JsonError::Interrupted);
        }
        match self.peek() {
            None => Err(invalid("syntax", self.i)),
            Some(b'n') => self.lit("null", Value::Null),
            Some(b't') => self.lit("true", Value::Bool(true)),
            Some(b'f') => self.lit("false", Value::Bool(false)),
            Some(b'"') => Ok(Value::Str(Rc::from(self.string()?.as_str()))),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(invalid("syntax", self.i)),
        }
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.i += 1;
            }
            Some(c) if c.is_ascii_digit() => {
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.i += 1;
                }
            }
            _ => return Err(invalid("syntax", self.i)),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            let fs = self.i;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
            if fs == self.i {
                return Err(invalid("syntax", self.i));
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            let es = self.i;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
            if es == self.i {
                return Err(invalid("syntax", self.i));
            }
        }
        match Decimal::parse_json(&self.src[start..self.i]) {
            Ok(Some(d)) => Ok(Value::num(d)),
            Ok(None) => Err(invalid("syntax", start)),
            Err(_) => Err(JsonError::Overflow),
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.i + 4 > self.s.len() {
            return Err(invalid("syntax", self.i));
        }
        let mut v = 0u32;
        for k in 0..4 {
            let c = self.s[self.i + k];
            let d = (c as char).to_digit(16).ok_or_else(|| invalid("syntax", self.i + k))?;
            v = v * 16 + d;
        }
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.i += 1; // opening quote
        let mut out = String::new();
        loop {
            let c = match self.peek() {
                None => return Err(invalid("syntax", self.i)),
                Some(c) => c,
            };
            match c {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    let esc_at = self.i;
                    self.i += 1;
                    let e = self.peek().ok_or_else(|| invalid("syntax", self.i))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let u = self.hex4()?;
                            if (0xD800..0xDC00).contains(&u) {
                                if self.s[self.i..].starts_with(b"\\u") {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        let cp = 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00);
                                        out.push(char::from_u32(cp).unwrap());
                                    } else {
                                        return Err(invalid("surrogate", esc_at));
                                    }
                                } else {
                                    return Err(invalid("surrogate", esc_at));
                                }
                            } else if (0xDC00..0xE000).contains(&u) {
                                return Err(invalid("surrogate", esc_at));
                            } else {
                                out.push(char::from_u32(u).unwrap());
                            }
                        }
                        _ => return Err(invalid("syntax", esc_at)),
                    }
                }
                c if c < 0x20 => return Err(invalid("syntax", self.i)),
                _ => {
                    // copy one UTF-8 scalar
                    let ch = self.src[self.i..].chars().next().unwrap();
                    out.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
    }

    fn array(&mut self) -> Result<Value, JsonError> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::array(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::array(items));
                }
                _ => return Err(invalid("syntax", self.i)),
            }
        }
    }

    fn object(&mut self) -> Result<Value, JsonError> {
        self.i += 1;
        let mut r = Record::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::record(r));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(invalid("syntax", self.i));
            }
            let key_at = self.i;
            let k = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(invalid("syntax", self.i));
            }
            self.i += 1;
            self.ws();
            let v = self.value()?;
            if r.contains(&k) {
                return Err(invalid("duplicate_key", key_at));
            }
            r.insert(Rc::from(k.as_str()), v);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::record(r));
                }
                _ => return Err(invalid("syntax", self.i)),
            }
        }
    }
}

/// Canonical string escaping (§3.4).
pub fn write_string(out: &mut String, s: &str) {
    if s.len() > 4096 {
        if crate::interrupt::poll_big(s.len()) {
            return; // partial output is discarded by the runtime
        }
        let escaped: usize = s
            .bytes()
            .map(|b| match b {
                b'"' | b'\\' | 8 | 12 | b'\n' | b'\r' | b'\t' => 2,
                0..=0x1f => 6,
                _ => 1,
            })
            .sum();
        if crate::interrupt::reserve(out.len() + escaped + 2) {
            return;
        }
        out.reserve(escaped + 2);
    }
    out.push('"');
    for (i, ch) in s.chars().enumerate() {
        if i & 0xffff == 0xffff && crate::interrupt::poll_big(65536) {
            return;
        }
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let v = c as u32;
                out.push_str("\\u00");
                out.push(HEX[(v >> 4) as usize] as char);
                out.push(HEX[(v & 15) as usize] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Canonical compact serialization. Caller guarantees the value is JSON.
pub fn dump(v: &Value) -> String {
    let mut out = String::new();
    dump_into(&mut out, v);
    out
}

pub fn dump_into(out: &mut String, v: &Value) {
    if crate::interrupt::poll() {
        return; // partial output is discarded by the runtime
    }
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Num(d) => out.push_str(&d.canonical()),
        Value::Str(s) => write_string(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                dump_into(out, x);
            }
            out.push(']');
        }
        Value::Record(r) => {
            out.push('{');
            for (i, (k, x)) in r.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                dump_into(out, x);
            }
            out.push('}');
        }
        other => panic!("dump of non-JSON kind {}", other.kind().name()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(s: &str) -> &'static str {
        match parse(s) {
            Err(JsonError::Invalid { reason, .. }) => reason,
            other => panic!("expected error, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_and_order() {
        let v = parse(" \n{\"b\":9007199254740993,\"a\":\"\\uD83D\\uDE00\"}\t").unwrap();
        assert_eq!(dump(&v), "{\"b\":9007199254740993,\"a\":\"😀\"}");
    }

    #[test]
    fn rejections() {
        assert_eq!(reason("{\"a\":1,\"\\u0061\":2}"), "duplicate_key");
        assert_eq!(reason("1 2"), "trailing");
        assert_eq!(reason("\u{FEFF}1"), "bom");
        assert_eq!(reason("\"\\uD800\""), "surrogate");
        assert_eq!(reason("[1,]"), "syntax");
        assert_eq!(reason("NaN"), "syntax");
        assert_eq!(reason("\"a\nb\""), "syntax");
        assert_eq!(reason(""), "syntax");
    }

    #[test]
    fn escaping() {
        let mut s = String::new();
        write_string(&mut s, "/é\u{2028}\u{0}\u{1f}\"");
        assert_eq!(s, "\"/é\u{2028}\\u0000\\u001f\\\"\"");
    }
}
