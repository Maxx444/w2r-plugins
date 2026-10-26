//! A small JSON reader for preset files, so the plugin needs no extra crate. It accepts strict
//! JSON (RFC 8259), refuses anything else, and limits nesting so a hostile file cannot overflow
//! the stack.

/// A parsed JSON value. Numbers are kept as `f64`; object keys keep their order.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// The value under `key` if this is an object that has it.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(items) => Some(items),
            _ => None,
        }
    }
}

/// Deepest nesting accepted; preset files need four levels.
const MAX_DEPTH: usize = 32;

/// Parses one JSON document (surrounding whitespace and a UTF-8 byte order mark allowed).
pub fn parse(text: &str) -> Option<Value> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value(0)?;
    p.ws();
    (p.i == p.b.len()).then_some(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self, w: &[u8]) -> bool {
        if self.b.get(self.i..self.i + w.len()) == Some(w) {
            self.i += w.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.ws();
        match self.peek()? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => self.string().map(Value::Str),
            b't' => self.word(b"true").then_some(Value::Bool(true)),
            b'f' => self.word(b"false").then_some(Value::Bool(false)),
            b'n' => self.word(b"null").then_some(Value::Null),
            _ => self.number(),
        }
    }

    fn object(&mut self, depth: usize) -> Option<Value> {
        self.i += 1; // '{'
        let mut fields = Vec::new();
        self.ws();
        if self.eat(b'}') {
            return Some(Value::Obj(fields));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return None;
            }
            let key = self.string()?;
            self.ws();
            if !self.eat(b':') {
                return None;
            }
            let v = self.value(depth + 1)?;
            fields.push((key, v));
            self.ws();
            if self.eat(b'}') {
                return Some(Value::Obj(fields));
            }
            if !self.eat(b',') {
                return None;
            }
        }
    }

    fn array(&mut self, depth: usize) -> Option<Value> {
        self.i += 1; // '['
        let mut items = Vec::new();
        self.ws();
        if self.eat(b']') {
            return Some(Value::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.ws();
            if self.eat(b']') {
                return Some(Value::Arr(items));
            }
            if !self.eat(b',') {
                return None;
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.b.get(self.i..self.i + 4)?;
        let s = core::str::from_utf8(digits).ok()?;
        let v = u32::from_str_radix(s, 16).ok()?;
        self.i += 4;
        Some(v)
    }

    fn string(&mut self) -> Option<String> {
        self.i += 1; // '"'
        let mut out = String::new();
        loop {
            let start = self.i;
            while let Some(c) = self.peek() {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.i += 1;
            }
            // The input is a &str and we only stopped on ASCII, so this slice is valid UTF-8.
            out.push_str(core::str::from_utf8(&self.b[start..self.i]).ok()?);
            match self.peek()? {
                b'"' => {
                    self.i += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.i += 1;
                    let e = self.peek()?;
                    self.i += 1;
                    let c = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xd800..0xdc00).contains(&hi) && self.word(b"\\u") {
                                let lo = self.hex4()?;
                                if (0xdc00..0xe000).contains(&lo) {
                                    char::from_u32(0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00))
                                        .unwrap_or('\u{fffd}')
                                } else {
                                    '\u{fffd}'
                                }
                            } else {
                                char::from_u32(hi).unwrap_or('\u{fffd}')
                            }
                        }
                        _ => return None,
                    };
                    out.push(c);
                }
                _ => return None, // a raw control character
            }
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.i;
        self.eat(b'-');
        let int_start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        let int = &self.b[int_start..self.i];
        if int.is_empty() || (int.len() > 1 && int[0] == b'0') {
            return None;
        }
        if self.eat(b'.') {
            let f = self.i;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            if self.i == f {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if !self.eat(b'+') {
                self.eat(b'-');
            }
            let e = self.i;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            if self.i == e {
                return None;
            }
        }
        let s = core::str::from_utf8(&self.b[start..self.i]).ok()?;
        s.parse().ok().map(Value::Num)
    }
}

/// `text` as a JSON string literal, quotes included.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_documents() {
        let v = parse(r#" { "a": [1, -2.5e1, true, null], "b": {"c": "x\"y\u00e9"} } "#).unwrap();
        assert_eq!(
            v.get("a").unwrap().as_array().unwrap(),
            &[
                Value::Num(1.0),
                Value::Num(-25.0),
                Value::Bool(true),
                Value::Null
            ]
        );
        assert_eq!(
            v.get("b").unwrap().get("c").unwrap().as_str(),
            Some("x\"y\u{e9}")
        );
    }

    #[test]
    fn accepts_a_byte_order_mark_and_surrogate_pairs() {
        let v = parse("\u{feff}\"\\ud83d\\ude00\"").unwrap();
        assert_eq!(v.as_str(), Some("\u{1f600}"));
    }

    #[test]
    fn refuses_malformed_input() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "01",
            "1.",
            "\"\\x\"",
            "tru",
            "[1] 2",
            "{'a':1}",
            "\"a\nb\"",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn limits_nesting() {
        let deep = "[".repeat(100) + &"]".repeat(100);
        assert_eq!(parse(&deep), None);
        let ok = "[".repeat(10) + &"]".repeat(10);
        assert!(parse(&ok).is_some());
    }

    #[test]
    fn quotes_round_trip() {
        let s = "a\"b\\c\nd\u{1}é";
        assert_eq!(parse(&quote(s)).unwrap().as_str(), Some(s));
    }
}
