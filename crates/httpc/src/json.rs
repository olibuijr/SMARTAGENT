//! Minimal hand-rolled JSON: parse to `Value`, escape strings for output.
//! Just enough for embeddings responses and metadata round-trips. No serde.

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
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
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
    out
}

pub fn parse(input: &str) -> Result<Value, String> {
    let bytes = input.as_bytes();
    let mut p = Parser { b: bytes, i: 0 };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.i != bytes.len() {
        return Err(format!("trailing data at byte {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.i))
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.lit("true", Value::Bool(true)),
            Some(b'f') => self.lit("false", Value::Bool(false)),
            Some(b'n') => self.lit("null", Value::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(format!("unexpected byte at {}", self.i)),
        }
    }

    fn lit(&mut self, word: &str, v: Value) -> Result<Value, String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(format!("bad literal at byte {}", self.i))
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.expect(b'{')?;
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::Obj(pairs));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let val = self.value()?;
            pairs.push((key, val));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Obj(pairs));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.i)),
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Arr(items));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.i)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated string".into()),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some(b'"') => out.push('"'),
                        Some(b'\\') => out.push('\\'),
                        Some(b'/') => out.push('/'),
                        Some(b'n') => out.push('\n'),
                        Some(b'r') => out.push('\r'),
                        Some(b't') => out.push('\t'),
                        Some(b'b') => out.push('\u{0008}'),
                        Some(b'f') => out.push('\u{000C}'),
                        Some(b'u') => {
                            self.i += 1;
                            let cp = self.hex4()?;
                            // Handle surrogate pairs.
                            let ch = if (0xD800..0xDC00).contains(&cp) {
                                if self.b[self.i..].starts_with(b"\\u") {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    let c = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    char::from_u32(c).ok_or("bad surrogate pair")?
                                } else {
                                    return Err("lone surrogate".into());
                                }
                            } else {
                                char::from_u32(cp).ok_or("bad codepoint")?
                            };
                            out.push(ch);
                            continue; // hex4 already advanced past the digits
                        }
                        _ => return Err(format!("bad escape at byte {}", self.i)),
                    }
                    self.i += 1;
                }
                Some(_) => {
                    // Bulk-copy the plain-ASCII run (validating from_utf8 on the
                    // WHOLE remaining buffer per char was O(n²) — a multi-MB
                    // base64 payload like a CDP screenshot took ~47s to parse).
                    let start = self.i;
                    while self.i < self.b.len() {
                        let c = self.b[self.i];
                        if c == b'"' || c == b'\\' || c >= 0x80 {
                            break;
                        }
                        self.i += 1;
                    }
                    if self.i > start {
                        // ASCII bytes are valid UTF-8 by construction.
                        out.push_str(std::str::from_utf8(&self.b[start..self.i]).map_err(|_| "bad utf8")?);
                        continue;
                    }
                    // Non-ASCII: validate only this char's bounded slice.
                    let end = (self.i + 4).min(self.b.len());
                    let c = match std::str::from_utf8(&self.b[self.i..end]) {
                        Ok(s) => s.chars().next().ok_or("empty")?,
                        // A trailing char can straddle `end`; from_utf8 tells us
                        // the valid prefix — take its first char if any.
                        Err(e) if e.valid_up_to() > 0 => {
                            std::str::from_utf8(&self.b[self.i..self.i + e.valid_up_to()])
                                .map_err(|_| "bad utf8")?
                                .chars()
                                .next()
                                .ok_or("empty")?
                        }
                        Err(_) => return Err("bad utf8".into()),
                    };
                    out.push(c);
                    self.i += c.len_utf8();
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        if self.i + 4 > self.b.len() {
            return Err("short \\u escape".into());
        }
        let s = std::str::from_utf8(&self.b[self.i..self.i + 4]).map_err(|_| "bad hex")?;
        let v = u32::from_str_radix(s, 16).map_err(|_| "bad hex")?;
        self.i += 4;
        Ok(v)
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while matches!(
            self.peek(),
            Some(c) if c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            self.i += 1;
        }
        let s = std::str::from_utf8(&self.b[start..self.i]).map_err(|_| "bad num")?;
        s.parse::<f64>()
            .map(Value::Num)
            .map_err(|e| format!("bad number '{s}': {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_embeddings_shape() {
        let v = parse(r#"{"data":[{"embedding":[0.1,-0.2,3e-1],"index":0}],"model":"m"}"#).unwrap();
        let emb = v.get("data").unwrap().as_arr().unwrap()[0]
            .get("embedding")
            .unwrap()
            .as_arr()
            .unwrap();
        assert_eq!(emb.len(), 3);
        assert!((emb[2].as_f64().unwrap() - 0.3).abs() < 1e-9);
    }

    #[test]
    fn escapes_round_trip() {
        let s = "he said \"hi\"\nline2\t\\";
        let parsed = parse(&format!("\"{}\"", escape(s))).unwrap();
        assert_eq!(parsed.as_str().unwrap(), s);
    }

    #[test]
    fn rejects_trailing() {
        assert!(parse("{}x").is_err());
    }

    #[test]
    fn unicode_escapes() {
        let v = parse(r#""é 😀""#).unwrap();
        assert_eq!(v.as_str().unwrap(), "é 😀");
    }

    #[test]
    fn ascii_runs_mixed_with_escapes_and_multibyte() {
        let v = parse(r#""abc\ndéf😀ghi\t""#).unwrap();
        assert_eq!(v.as_str().unwrap(), "abc\ndéf😀ghi\t");
    }

    #[test]
    fn large_string_parses_fast() {
        // Regression for the O(n²) per-char from_utf8: a 4MB base64-ish string
        // (CDP screenshot shape). Quadratic behavior turns this test from
        // milliseconds into ~20s — the suite timeout is the guard.
        let big = "A1b2C3d4".repeat(512 * 1024);
        let v = parse(&format!(r#"{{"data":"{big}"}}"#)).unwrap();
        assert_eq!(v.get("data").unwrap().as_str().unwrap().len(), big.len());
    }
}
