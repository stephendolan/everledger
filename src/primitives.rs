use crate::document::Document;
use std::{collections::HashMap, sync::OnceLock};

pub fn float_repr(n: f64) -> String {
    if n.is_nan() {
        return "nan".into();
    }
    if n.is_infinite() {
        return if n < 0.0 { "-inf" } else { "inf" }.into();
    }
    if n == 0.0 {
        return if n.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let negative = n.is_sign_negative();
    let n = n.abs();
    let shortest = format!("{n:e}");
    let precision = shortest
        .split('e')
        .next()
        .unwrap_or("")
        .chars()
        .filter(char::is_ascii_digit)
        .count()
        .saturating_sub(1);
    let rounded = format!("{n:.precision$e}");
    let scientific = if rounded
        .parse::<f64>()
        .is_ok_and(|v| v.to_bits() == n.to_bits())
    {
        rounded
    } else {
        shortest
    };
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("17 digits round trip f64");
    let exponent: i32 = exponent.parse().unwrap();
    let sign = if negative { "-" } else { "" };
    if !(-4..16).contains(&exponent) {
        return format!(
            "{sign}{mantissa}e{}{exp:02}",
            if exponent >= 0 { "+" } else { "-" },
            exp = exponent.abs()
        );
    }
    let digits = mantissa.replace('.', "");
    let point = exponent + 1;
    if point <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!(
            "{sign}{digits}{}.0",
            "0".repeat(point as usize - digits.len())
        )
    } else {
        format!(
            "{sign}{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    }
}
#[derive(Clone, Debug, serde::Serialize)]
#[serde(untagged)]
pub enum Scalar {
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
}
impl<'de> serde::Deserialize<'de> for Scalar {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ScalarVisitor;
        impl<'de> serde::de::Visitor<'de> for ScalarVisitor {
            type Value = Scalar;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a boolean, number, or string")
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Scalar, E> {
                Ok(Scalar::Bool(value))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Scalar, E> {
                Ok(Scalar::Integer(value))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Scalar, E> {
                Ok(i64::try_from(value)
                    .map(Scalar::Integer)
                    .unwrap_or_else(|_| Scalar::Float(value as f64)))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Scalar, E> {
                Ok(Scalar::Float(value))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Scalar, E> {
                Ok(Scalar::Text(value.into()))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Scalar, E> {
                Ok(Scalar::Text(value))
            }
        }
        deserializer.deserialize_any(ScalarVisitor)
    }
}
impl Scalar {
    pub fn python_str(&self) -> String {
        match self {
            Self::Bool(b) => if *b { "True" } else { "False" }.into(),
            Self::Integer(n) => n.to_string(),
            Self::Float(n) => float_repr(*n),
            Self::Text(s) => s.clone(),
        }
    }
}
pub fn human(value: Option<&Scalar>) -> String {
    match value {
        None => "unknown".into(),
        Some(Scalar::Bool(b)) => if *b { "yes" } else { "no" }.into(),
        Some(Scalar::Text(s)) if s.is_empty() => "unknown".into(),
        Some(v) => v.python_str(),
    }
}
pub fn py_str(value: &Document) -> crate::Result<String> {
    match value {
        Document::Null => Ok("None".into()),
        Document::Bool(b) => Ok(if *b { "True" } else { "False" }.into()),
        Document::String(s) => Ok(s.clone()),
        Document::Number(n) => Ok(n.text()),
        Document::Array(_) | Document::Object(_) => {
            Err(crate::Failure::shape("scalar", "expected scalar value"))
        }
    }
}
pub fn copper_money(v: Option<crate::Copper>) -> String {
    let Some(crate::Copper(v)) = v else {
        return "unknown".into();
    };
    let n = v.unsigned_abs();
    let parts = [(n / 10000, "g"), ((n % 10000) / 100, "s"), (n % 100, "c")]
        .into_iter()
        .filter(|(n, _)| *n != 0)
        .map(|(n, u)| format!("{n}{u}"))
        .collect::<Vec<_>>();
    format!(
        "{}{}",
        if v < 0 { "-" } else { "" },
        if parts.is_empty() {
            "0c".into()
        } else {
            parts.join(" ")
        }
    )
}
type Mappings = HashMap<char, String>;
fn mappings() -> &'static Mappings {
    static MAP: OnceLock<Mappings> = OnceLock::new();
    MAP.get_or_init(|| {
        include_str!("unicode.tsv")
            .lines()
            .map(|l| {
                let c = l.split('\t').collect::<Vec<_>>();
                let decode = |s: &str| {
                    s.split(',')
                        .map(|n| char::from_u32(u32::from_str_radix(n, 16).unwrap()).unwrap())
                        .collect::<String>()
                };
                (
                    char::from_u32(u32::from_str_radix(c[0], 16).unwrap()).unwrap(),
                    decode(c[1]),
                )
            })
            .collect()
    })
}
pub fn casefold(s: &str) -> String {
    s.chars()
        .map(|c| mappings().get(&c).cloned().unwrap_or_else(|| c.to_string()))
        .collect()
}
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}
pub fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
pub fn urlencode(params: &[(String, Document)]) -> crate::Result<String> {
    params
        .iter()
        .map(|(k, v)| {
            Ok(format!(
                "{}={}",
                quote(k).replace("%20", "+"),
                quote(&py_str(v)?).replace("%20", "+")
            ))
        })
        .collect::<crate::Result<Vec<_>>>()
        .map(|parts| parts.join("&"))
}
const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for s in bytes.chunks(3) {
        let n = ((s[0] as u32) << 16)
            | ((s.get(1).copied().unwrap_or(0) as u32) << 8)
            | s.get(2).copied().unwrap_or(0) as u32;
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push(if s.len() > 1 {
            B64[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if s.len() > 2 {
            B64[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base64Error {
    InvalidAlphabet,
    InvalidPadding,
}
impl std::fmt::Display for Base64Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidAlphabet => "invalid base64 alphabet",
            Self::InvalidPadding => "invalid base64 padding",
        })
    }
}
impl std::error::Error for Base64Error {}
pub fn base64_decode(s: &str) -> std::result::Result<Vec<u8>, Base64Error> {
    let mut out = vec![];
    let mut acc = 0u32;
    let mut count = 0usize;
    let mut pad = 0usize;
    for b in s.bytes() {
        if b == b'=' {
            pad += 1;
            continue;
        }
        if pad > 0 {
            return Err(Base64Error::InvalidPadding);
        }
        let b = match b {
            b'+' => b'-',
            b'/' => b'_',
            b => b,
        };
        let n = B64
            .iter()
            .position(|&v| v == b)
            .ok_or(Base64Error::InvalidAlphabet)?;
        acc = (acc << 6) | n as u32;
        count += 1;
        if count % 4 == 2 {
            out.push((acc >> 4) as u8);
        }
        if count % 4 == 3 {
            out.push((acc >> 2) as u8);
        }
        if count.is_multiple_of(4) {
            out.push(acc as u8);
        }
    }
    if (count % 4 == 1)
        || (count % 4 == 2 && pad != 2)
        || (count % 4 == 3 && pad != 1)
        || (count.is_multiple_of(4) && pad > 0)
    {
        return Err(Base64Error::InvalidPadding);
    }
    Ok(out)
}
pub fn rounded_copper(n: f64) -> crate::Result<crate::Copper> {
    let n = n.round_ties_even();
    if !n.is_finite() || n < i64::MIN as f64 || n >= i64::MAX as f64 {
        return Err(crate::Failure::error("copper amount out of range"));
    }
    Ok(crate::Copper(n as i64))
}
