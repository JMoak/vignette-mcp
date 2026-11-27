//! Just enough JSON for one protocol line: a strict reader that agrees with `JSON.parse` on
//! everything the console sends, and the escaping `JSON.stringify` uses for the text it echoes.

const DEEPEST: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// A field of an object. A name given twice means its last value, as in `JSON.parse`.
    pub fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Object(fields) => fields.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Option<Value> {
    let mut reader = Reader { bytes: text.as_bytes(), at: 0 };
    let value = reader.value(0)?;
    reader.skip_space();
    (reader.at == reader.bytes.len()).then_some(value)
}

/// Writes `text` as a JSON string, escaped the way `JSON.stringify` escapes it.
pub fn write_text(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(control))),
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Writes a number the way `JSON.stringify` does for the numbers this protocol carries.
pub fn write_number(out: &mut String, number: f64) {
    if number.fract() == 0.0 && number.abs() < 1e21 {
        out.push_str(&format!("{}", number as i128));
    } else {
        out.push_str(&format!("{number}"));
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, expected: u8) -> Option<()> {
        (self.peek() == Some(expected)).then(|| self.at += 1)
    }

    fn word(&mut self, word: &[u8], value: Value) -> Option<Value> {
        let end = self.at.checked_add(word.len())?;
        (self.bytes.get(self.at..end) == Some(word)).then(|| {
            self.at = end;
            value
        })
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        self.skip_space();
        match self.peek()? {
            b'{' if depth < DEEPEST => self.object(depth + 1),
            b'[' if depth < DEEPEST => self.list(depth + 1),
            b'"' => self.text().map(Value::Text),
            b't' => self.word(b"true", Value::Bool(true)),
            b'f' => self.word(b"false", Value::Bool(false)),
            b'n' => self.word(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn object(&mut self, depth: usize) -> Option<Value> {
        self.eat(b'{')?;
        let mut fields = Vec::new();
        self.skip_space();
        if self.eat(b'}').is_some() {
            return Some(Value::Object(fields));
        }
        loop {
            self.skip_space();
            let name = self.text()?;
            self.skip_space();
            self.eat(b':')?;
            fields.push((name, self.value(depth)?));
            self.skip_space();
            if self.eat(b'}').is_some() {
                return Some(Value::Object(fields));
            }
            self.eat(b',')?;
        }
    }

    fn list(&mut self, depth: usize) -> Option<Value> {
        self.eat(b'[')?;
        let mut items = Vec::new();
        self.skip_space();
        if self.eat(b']').is_some() {
            return Some(Value::List(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.skip_space();
            if self.eat(b']').is_some() {
                return Some(Value::List(items));
            }
            self.eat(b',')?;
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.at;
        let _ = self.eat(b'-');
        if self.eat(b'0').is_none() && self.digits() == 0 {
            return None;
        }
        if self.eat(b'.').is_some() && self.digits() == 0 {
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return None;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).ok()?;
        text.parse().ok().map(Value::Number)
    }

    fn text(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut text = String::new();
        loop {
            let start = self.at;
            while !matches!(self.peek()?, b'"' | b'\\' | 0..=0x1f) {
                self.at += 1;
            }
            text.push_str(std::str::from_utf8(&self.bytes[start..self.at]).ok()?);
            match self.peek()? {
                b'"' => {
                    self.at += 1;
                    return Some(text);
                }
                b'\\' => {
                    self.at += 1;
                    text.push(self.escaped()?);
                }
                _ => return None,
            }
        }
    }

    fn escaped(&mut self) -> Option<char> {
        let letter = self.peek()?;
        self.at += 1;
        Some(match letter {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode(),
            _ => return None,
        })
    }

    // A lone surrogate, which `JSON.parse` would keep, is refused: Rust text cannot hold one, and
    // nothing the console sends contains one.
    fn unicode(&mut self) -> Option<char> {
        let high = self.hex4()?;
        if !(0xd800..0xdc00).contains(&high) {
            return char::from_u32(high);
        }
        self.eat(b'\\')?;
        self.eat(b'u')?;
        let low = self.hex4()?;
        if !(0xdc00..0xe000).contains(&low) {
            return None;
        }
        char::from_u32(0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00))
    }

    fn hex4(&mut self) -> Option<u32> {
        let end = self.at.checked_add(4)?;
        let digits = std::str::from_utf8(self.bytes.get(self.at..end)?).ok()?;
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        self.at = end;
        u32::from_str_radix(digits, 16).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    #[test]
    fn reads_what_the_console_writes() {
        let line = r#" {"v":1,"kind":"grant","grantId":"g1","expiresAt":1764115200000,
            "display":{"x":-1920,"y":0,"width":1920,"height":1080,"scaleFactor":1.25},"versions":[1, 2e0]} "#;
        let value = parse(line).expect("a valid line");
        assert_eq!(value.field("kind"), Some(&text("grant")));
        assert_eq!(value.field("expiresAt"), Some(&Value::Number(1_764_115_200_000.0)));
        let display = value.field("display").expect("a display");
        assert_eq!(display.field("x"), Some(&Value::Number(-1920.0)));
        assert_eq!(display.field("scaleFactor"), Some(&Value::Number(1.25)));
        assert_eq!(value.field("versions"), Some(&Value::List(vec![Value::Number(1.0), Value::Number(2.0)])));
    }

    #[test]
    fn decodes_escapes_and_keeps_the_last_of_a_repeated_name() {
        let escaped = parse(r#""\"\\\/\b\f\n\r\té😀""#);
        assert_eq!(escaped, Some(text("\"\\/\u{8}\u{c}\n\r\té😀")));
        let value = parse(r#"{"a":"first","a":"xA","b":[true,false,null]}"#).expect("valid");
        assert_eq!(value.field("a"), Some(&text("xA")));
        assert_eq!(
            value.field("b"),
            Some(&Value::List(vec![Value::Bool(true), Value::Bool(false), Value::Null]))
        );
    }

    #[test]
    fn refuses_what_json_parse_refuses() {
        let refused = [
            "",
            "{",
            r#"{"v":1,}"#,
            r#"{"v":01}"#,
            r#"{"v":1.}"#,
            r#"{"v":.5}"#,
            r#"{"v":+1}"#,
            r#"{"v":1e}"#,
            r#"{'v':1}"#,
            r#"{"v":1} x"#,
            r#"{"v":tru}"#,
            "\"a\tb\"",
            r#""\x""#,
            r#""\u12g4""#,
            "[1 2]",
        ];
        for line in refused {
            assert_eq!(parse(line), None, "{line}");
        }
    }

    #[test]
    fn refuses_a_lone_surrogate_and_nesting_deeper_than_a_line_needs() {
        assert_eq!(parse(r#""\ud83d""#), None);
        assert_eq!(parse(r#""\ude00""#), None);
        assert!(parse(&format!("{}1{}", "[".repeat(8), "]".repeat(8))).is_some());
        assert_eq!(parse(&format!("{}1{}", "[".repeat(9), "]".repeat(9))), None);
    }

    #[test]
    fn writes_text_and_numbers_as_json_stringify_does() {
        let mut out = String::new();
        write_text(&mut out, "a\"b\\c\nd\u{1}é");
        assert_eq!(out, r#""a\"b\\c\nd\u0001é""#);
        let mut out = String::new();
        for number in [1_764_115_200_000.0, 2.0, 0.5, -3.0] {
            write_number(&mut out, number);
            out.push(' ');
        }
        assert_eq!(out, "1764115200000 2 0.5 -3 ");
    }
}
