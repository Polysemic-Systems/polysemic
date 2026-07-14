//! polysemic-core — the univocal interior.
//!
//! Everything *inside* the seam is strict: one `Value`, one meaning, one
//! strict parser. The polysemy lives in `digest`, at the boundary, where it
//! belongs. This parser refuses everything non-standard on purpose — it is
//! the measuring stick the repair passes in `digest` are judged against.

use std::collections::BTreeMap;
use std::fmt;

/// A JSON value. `BTreeMap` keeps object keys ordered and output deterministic.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(BTreeMap<String, Value>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::Arr(_) => "array",
            Value::Obj(_) => "object",
        }
    }

    pub fn as_obj(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Obj(m) => Some(m),
            _ => None,
        }
    }
}

/// A parse failure, with byte offset. The strict parser does not repair —
/// it reports. Repair is a separate, *named* act (see `digest::Repair`).
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub at: usize,
    pub msg: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parse error at byte {}: {}", self.at, self.msg)
    }
}

impl std::error::Error for ParseError {}

/// Strictly parse a JSON document. Trailing content is an error.
pub fn parse(input: &str) -> Result<Value, ParseError> {
    let mut p = Parser {
        b: input.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(p.err("trailing characters after value"));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, msg: &str) -> ParseError {
        ParseError {
            at: self.i,
            msg: msg.to_string(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn expect_lit(&mut self, lit: &str, v: Value) -> Result<Value, ParseError> {
        if self.b[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            Ok(v)
        } else {
            Err(self.err(&format!("expected `{lit}`")))
        }
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            Some(b'n') => self.expect_lit("null", Value::Null),
            Some(b't') => self.expect_lit("true", Value::Bool(true)),
            Some(b'f') => self.expect_lit("false", Value::Bool(false)),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(_) => Err(self.err("unexpected character")),
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.i += 1;
                if matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    return Err(self.err("leading zero in number"));
                }
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.i += 1;
                }
            }
            _ => return Err(self.err("expected digit in number")),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(self.err("expected digit after decimal point"));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(self.err("expected digit in exponent"));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        let s = std::str::from_utf8(&self.b[start..self.i]).unwrap();
        match s.parse::<f64>() {
            Ok(number) if number.is_finite() => Ok(Value::Num(number)),
            _ => Err(ParseError {
                at: start,
                msg: format!("invalid or non-finite number `{s}`"),
            }),
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        // assumes b[i] == b'"'
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.peek() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.i += 1;
                    // input was valid UTF-8; raw bytes copied verbatim stay valid
                    return String::from_utf8(out).map_err(|_| self.err("invalid utf-8 in string"));
                }
                Some(b'\\') => {
                    self.i += 1;
                    let esc = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
                    self.i += 1;
                    match esc {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let cp = self.hex4()?;
                            let ch = if (0xD800..=0xDBFF).contains(&cp) {
                                // high surrogate: expect \uXXXX low surrogate
                                if self.peek() == Some(b'\\')
                                    && self.b.get(self.i + 1) == Some(&b'u')
                                {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if !(0xDC00..=0xDFFF).contains(&lo) {
                                        return Err(self.err(
                                            "high surrogate is not followed by a low surrogate",
                                        ));
                                    }
                                    let c = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    char::from_u32(c)
                                        .ok_or_else(|| self.err("invalid unicode scalar value"))?
                                } else {
                                    return Err(self.err("unpaired high surrogate"));
                                }
                            } else if (0xDC00..=0xDFFF).contains(&cp) {
                                return Err(self.err("unpaired low surrogate"));
                            } else {
                                char::from_u32(cp)
                                    .ok_or_else(|| self.err("invalid unicode scalar value"))?
                            };
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(self.err("invalid escape")),
                    }
                }
                Some(c) if c < 0x20 => return Err(self.err("raw control character in string")),
                Some(c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let mut cp: u32 = 0;
        for _ in 0..4 {
            let c = self
                .peek()
                .ok_or_else(|| self.err("truncated \\u escape"))?;
            let d = match c {
                b'0'..=b'9' => (c - b'0') as u32,
                b'a'..=b'f' => (c - b'a' + 10) as u32,
                b'A'..=b'F' => (c - b'A' + 10) as u32,
                _ => return Err(self.err("invalid hex digit in \\u escape")),
            };
            cp = cp * 16 + d;
            self.i += 1;
        }
        Ok(cp)
    }

    fn array(&mut self) -> Result<Value, ParseError> {
        self.i += 1; // '['
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Arr(items));
                }
                _ => return Err(self.err("expected `,` or `]` in array")),
            }
        }
    }

    fn object(&mut self) -> Result<Value, ParseError> {
        self.i += 1; // '{'
        let mut map = BTreeMap::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::Obj(map));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected string key"));
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected `:` after key"));
            }
            self.i += 1;
            self.ws();
            let val = self.value()?;
            if map.insert(key.clone(), val).is_some() {
                return Err(self.err(&format!("duplicate object key {key:?}")));
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Obj(map));
                }
                _ => return Err(self.err("expected `,` or `}` in object")),
            }
        }
    }
}

/// Serialize a `Value` back to compact JSON.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Num(n) => {
                if n.fract() == 0.0 && n.is_finite() && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{n}")
                }
            }
            Value::Str(s) => write_json_string(f, s),
            Value::Arr(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Value::Obj(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write_json_string(f, k)?;
                    write!(f, ":{v}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

fn write_json_string(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    write!(f, "\"")?;
    for c in s.chars() {
        match c {
            '"' => write!(f, "\\\"")?,
            '\\' => write!(f, "\\\\")?,
            '\n' => write!(f, "\\n")?,
            '\r' => write!(f, "\\r")?,
            '\t' => write!(f, "\\t")?,
            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
            c => write!(f, "{c}")?,
        }
    }
    write!(f, "\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_roundtrips() {
        let src = r#"{"a":[1,2.5,true,null],"b":{"c":"hi\nthere"}}"#;
        let v = parse(src).unwrap();
        assert_eq!(v.to_string(), src);
    }

    #[test]
    fn strict_parser_refuses_trailing_comma() {
        assert!(parse(r#"{"a":1,}"#).is_err());
    }

    #[test]
    fn handles_unicode_escapes() {
        let v = parse(r#""\u00e9 \ud83d\ude00""#).unwrap();
        assert_eq!(v, Value::Str("é 😀".to_string()));
    }

    #[test]
    fn rejects_duplicate_object_keys() {
        let error = parse(r#"{"qty":2,"qty":3}"#).unwrap_err();
        assert!(error.msg.contains("duplicate object key"));
    }

    #[test]
    fn rejects_non_json_and_non_finite_numbers() {
        for input in ["-", "01", "2.", "1e", "1e+", "1e999"] {
            assert!(parse(input).is_err(), "accepted invalid number {input:?}");
        }
    }

    #[test]
    fn rejects_unpaired_surrogates_without_panicking() {
        for input in [r#""\ud800""#, r#""\ud800\u0041""#, r#""\udc00""#] {
            assert!(parse(input).is_err(), "accepted invalid escape {input:?}");
        }
    }
}
