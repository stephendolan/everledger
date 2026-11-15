use crate::{Failure, Result};
use serde::{Serialize, de::DeserializeOwned};

#[derive(Clone, Debug)]
pub enum Number {
    Integer(String),
    Float(f64),
}
impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Integer(a), Self::Float(b)) | (Self::Float(b), Self::Integer(a)) => {
                b.is_finite()
                    && b.fract() == 0.0
                    && *a
                        == if *b == 0.0 {
                            "0".into()
                        } else {
                            format!("{b:.0}")
                        }
            }
        }
    }
}
impl Number {
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Integer(s) => s.parse().unwrap_or(f64::INFINITY),
            Self::Float(n) => *n,
        }
    }
    pub fn text(&self) -> String {
        match self {
            Self::Integer(s) => s.clone(),
            Self::Float(n) => crate::primitives::float_repr(*n),
        }
    }
}
#[derive(Clone, Debug)]
pub enum Document {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Self>),
    Object(Vec<(String, Self)>),
}
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::Bool(a), Self::Number(b)) | (Self::Number(b), Self::Bool(a)) => {
                b.as_f64() == f64::from(u8::from(*a))
            }
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v)| b.iter().any(|(key, value)| k == key && v == value))
            }
            _ => false,
        }
    }
}
impl Document {
    pub fn object(entries: impl IntoIterator<Item = (impl Into<String>, Self)>) -> Self {
        let mut values: Vec<(String, Self)> = vec![];
        for (k, v) in entries {
            let k = k.into();
            if let Some((_, old)) = values.iter_mut().find(|(key, _)| *key == k) {
                *old = v;
            } else {
                values.push((k, v));
            }
        }
        Self::Object(values)
    }
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Self>) -> Result<()> {
        let key = key.into();
        let value = value.into();
        if let Self::Object(v) = self {
            if let Some((_, old)) = v.iter_mut().find(|(k, _)| *k == key) {
                *old = value;
            } else {
                v.push((key, value));
            }
        } else {
            return Err(Failure::shape("document update", "expected object"));
        }
        Ok(())
    }
    pub fn set_default(&mut self, key: impl Into<String>, value: impl Into<Self>) -> Result<()> {
        let key = key.into();
        if self.get(&key).is_none() {
            self.set(key, value)?;
        }
        Ok(())
    }
    pub fn remove(&mut self, key: &str) -> Option<Self> {
        if let Self::Object(v) = self {
            v.iter().position(|(k, _)| k == key).map(|i| v.remove(i).1)
        } else {
            None
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_array(&self) -> Option<&[Self]> {
        if let Self::Array(a) = self {
            Some(a)
        } else {
            None
        }
    }
    pub fn as_object(&self) -> Option<&[(String, Self)]> {
        if let Self::Object(a) = self {
            Some(a)
        } else {
            None
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(n.as_f64()),
            Self::Bool(b) => Some(u8::from(*b) as f64),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Number(Number::Integer(n)) => n.parse().ok(),
            _ => None,
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::String(s) => !s.is_empty(),
            Self::Array(a) => !a.is_empty(),
            Self::Object(o) => !o.is_empty(),
            Self::Number(n) => n.as_f64() != 0.0,
        }
    }
    pub fn decode<T: DeserializeOwned>(&self, boundary: &str) -> Result<T> {
        serde_json::from_str(&dumps(self, JsonMode::Stdout))
            .map_err(|e| Failure::shape(boundary, e))
    }
    pub fn from_typed<T: Serialize>(value: &T) -> Result<Self> {
        parse(&serde_json::to_string(value).map_err(|e| Failure::error(e.to_string()))?)
    }
}
impl From<&str> for Document {
    fn from(v: &str) -> Self {
        Self::String(v.into())
    }
}
impl From<String> for Document {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}
impl From<bool> for Document {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<i64> for Document {
    fn from(v: i64) -> Self {
        Self::Number(Number::Integer(v.to_string()))
    }
}
impl From<usize> for Document {
    fn from(v: usize) -> Self {
        Self::Number(Number::Integer(v.to_string()))
    }
}
impl From<f64> for Document {
    fn from(v: f64) -> Self {
        Self::Number(Number::Float(v))
    }
}
impl<T: Into<Document>> From<Option<T>> for Document {
    fn from(v: Option<T>) -> Self {
        v.map(Into::into).unwrap_or(Self::Null)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum JsonMode {
    Stdout,
    Error,
    Cursor,
    Catalogue,
}
fn string(s: &str, ascii: bool) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c < ' ' || (ascii && !(' '..='~').contains(&c)) => {
                for unit in c.encode_utf16(&mut [0; 2]) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
pub fn dumps(doc: &Document, mode: JsonMode) -> String {
    fn render(d: &Document, mode: JsonMode, depth: usize) -> String {
        let ascii = matches!(mode, JsonMode::Error | JsonMode::Cursor);
        let indent = matches!(mode, JsonMode::Catalogue);
        let compact = matches!(mode, JsonMode::Cursor);
        let items = match d {
            Document::Null => return "null".into(),
            Document::Bool(b) => return b.to_string(),
            Document::Number(Number::Float(n)) if !n.is_finite() => {
                return if n.is_nan() {
                    "NaN"
                } else if n.is_sign_negative() {
                    "-Infinity"
                } else {
                    "Infinity"
                }
                .into();
            }
            Document::Number(n) => return n.text(),
            Document::String(s) => return string(s, ascii),
            Document::Array(a) => a
                .iter()
                .map(|x| render(x, mode, depth + 1))
                .collect::<Vec<_>>(),
            Document::Object(o) => o
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}:{}{}",
                        string(k, ascii),
                        if compact { "" } else { " " },
                        render(v, mode, depth + 1)
                    )
                })
                .collect(),
        };
        let (open, close) = if matches!(d, Document::Array(_)) {
            ('[', ']')
        } else {
            ('{', '}')
        };
        if items.is_empty() {
            return format!("{open}{close}");
        }
        if indent {
            format!(
                "{open}\n{}{}\n{}{close}",
                " ".repeat(depth + 1),
                items.join(&format!(",\n{}", " ".repeat(depth + 1))),
                " ".repeat(depth)
            )
        } else {
            format!(
                "{open}{}{close}",
                items.join(if compact { "," } else { ", " })
            )
        }
    }
    render(doc, mode, 0)
}
fn valid_number(text: &str) -> bool {
    let mut bytes = text.as_bytes().iter().copied().peekable();
    if bytes.peek() == Some(&b'-') {
        bytes.next();
    }
    match bytes.next() {
        Some(b'0') => {}
        Some(b'1'..=b'9') => {
            while bytes.peek().is_some_and(u8::is_ascii_digit) {
                bytes.next();
            }
        }
        _ => return false,
    }
    if bytes.peek() == Some(&b'.') {
        bytes.next();
        if !bytes.next().is_some_and(|b| b.is_ascii_digit()) {
            return false;
        }
        while bytes.peek().is_some_and(u8::is_ascii_digit) {
            bytes.next();
        }
    }
    if matches!(bytes.peek(), Some(b'e' | b'E')) {
        bytes.next();
        if matches!(bytes.peek(), Some(b'+' | b'-')) {
            bytes.next();
        }
        if !bytes.next().is_some_and(|b| b.is_ascii_digit()) {
            return false;
        }
        while bytes.peek().is_some_and(u8::is_ascii_digit) {
            bytes.next();
        }
    }
    bytes.next().is_none()
}
pub fn parse(text: &str) -> Result<Document> {
    struct Parser<'a> {
        s: &'a str,
        pos: usize,
    }
    impl Parser<'_> {
        fn ws(&mut self) {
            while self
                .s
                .as_bytes()
                .get(self.pos)
                .is_some_and(|b| b" \n\r\t".contains(b))
            {
                self.pos += 1;
            }
        }
        fn take(&mut self, b: u8) -> bool {
            self.ws();
            if self.s.as_bytes().get(self.pos) == Some(&b) {
                self.pos += 1;
                true
            } else {
                false
            }
        }
        fn err(&self) -> Failure {
            Failure::error(format!("invalid JSON at byte {}", self.pos))
        }
        fn string(&mut self) -> Result<String> {
            self.ws();
            let start = self.pos;
            if !self.take(b'"') {
                return Err(self.err());
            }
            let mut escaped = false;
            while let Some(&b) = self.s.as_bytes().get(self.pos) {
                self.pos += 1;
                if b == b'"' && !escaped {
                    return serde_json::from_str(&self.s[start..self.pos]).map_err(|_| self.err());
                }
                escaped = b == b'\\' && !escaped;
            }
            Err(self.err())
        }
        fn value(&mut self, depth: usize) -> Result<Document> {
            if depth > 256 {
                return Err(Failure::error("JSON nesting exceeds supported depth"));
            }
            self.ws();
            let b = *self.s.as_bytes().get(self.pos).ok_or_else(|| self.err())?;
            match b {
                b'"' => Ok(Document::String(self.string()?)),
                b'{' => {
                    self.pos += 1;
                    let mut d = Document::Object(vec![]);
                    if self.take(b'}') {
                        return Ok(d);
                    }
                    loop {
                        let k = self.string()?;
                        if !self.take(b':') {
                            return Err(self.err());
                        }
                        let v = self.value(depth + 1)?;
                        d.set(k, v)?;
                        if self.take(b'}') {
                            break;
                        }
                        if !self.take(b',') {
                            return Err(self.err());
                        }
                    }
                    Ok(d)
                }
                b'[' => {
                    self.pos += 1;
                    let mut a = vec![];
                    if self.take(b']') {
                        return Ok(Document::Array(a));
                    }
                    loop {
                        a.push(self.value(depth + 1)?);
                        if self.take(b']') {
                            break;
                        }
                        if !self.take(b',') {
                            return Err(self.err());
                        }
                    }
                    Ok(Document::Array(a))
                }
                _ => {
                    for (s, v) in [
                        ("null", Document::Null),
                        ("true", Document::Bool(true)),
                        ("false", Document::Bool(false)),
                        ("NaN", Document::from(f64::NAN)),
                        ("Infinity", Document::from(f64::INFINITY)),
                        ("-Infinity", Document::from(f64::NEG_INFINITY)),
                    ] {
                        if self.s[self.pos..].starts_with(s) {
                            self.pos += s.len();
                            return Ok(v);
                        }
                    }
                    let start = self.pos;
                    while self
                        .s
                        .as_bytes()
                        .get(self.pos)
                        .is_some_and(|b| b"-+0123456789.eE".contains(b))
                    {
                        self.pos += 1;
                    }
                    let raw = &self.s[start..self.pos];
                    if !valid_number(raw) {
                        return Err(self.err());
                    }
                    if raw.contains(['.', 'e', 'E']) {
                        Ok(Document::from(raw.parse::<f64>().map_err(|_| self.err())?))
                    } else {
                        Ok(Document::Number(Number::Integer(
                            if raw == "-0" { "0" } else { raw }.into(),
                        )))
                    }
                }
            }
        }
    }
    let mut p = Parser { s: text, pos: 0 };
    let d = p.value(0)?;
    p.ws();
    if p.pos != text.len() {
        return Err(p.err());
    }
    Ok(d)
}
