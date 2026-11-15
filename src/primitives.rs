use crate::document::{Document, Number};
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
    let mut scientific = String::new();
    for precision in 0..17 {
        let text = format!("{n:.precision$e}");
        if text
            .parse::<f64>()
            .is_ok_and(|v| v.to_bits() == n.to_bits())
        {
            scientific = text;
            break;
        }
    }
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
pub fn round(n: f64, digits: i32) -> f64 {
    if !n.is_finite() || digits > 323 {
        return n;
    }
    if digits < -308 {
        return 0.0f64.copysign(n);
    }
    if digits >= 0 {
        format!("{:.*}", digits as usize, n).parse().unwrap_or(n)
    } else {
        let negative = n.is_sign_negative();
        let n = n.abs();
        let zeros = (-digits) as usize;
        let integer = format!("{:.0}", n.trunc());
        let padded = format!("{:0>width$}", integer, width = zeros + 1);
        let cut = padded.len() - zeros;
        let (keep, tail) = padded.split_at(cut);
        let up = tail.as_bytes()[0] > b'5'
            || (tail.as_bytes()[0] == b'5'
                && (tail.as_bytes()[1..].iter().any(|&b| b != b'0')
                    || n.fract() != 0.0
                    || ((keep.as_bytes()[keep.len() - 1] - b'0') % 2 == 1)));
        let mut keep = keep.as_bytes().to_vec();
        if up {
            let mut carry = true;
            for b in keep.iter_mut().rev() {
                if *b == b'9' {
                    *b = b'0';
                } else {
                    *b += 1;
                    carry = false;
                    break;
                }
            }
            if carry {
                keep.insert(0, b'1');
            }
        }
        let text = format!(
            "{}{}{}",
            if negative { "-" } else { "" },
            String::from_utf8(keep).unwrap(),
            "0".repeat(zeros)
        );
        text.parse().unwrap_or(n)
    }
}
pub fn py_str(v: &Document) -> String {
    match v {
        Document::Null => "None".into(),
        Document::Bool(b) => if *b { "True" } else { "False" }.into(),
        Document::String(s) => s.clone(),
        Document::Number(n) => n.text(),
        Document::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Document::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", py_repr(&Document::from(k.clone())), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
fn py_repr(v: &Document) -> String {
    if let Document::String(s) = v {
        let quote = if s.contains('\'') && !s.contains('"') {
            '"'
        } else {
            '\''
        };
        let mut out = String::new();
        out.push(quote);
        for c in s.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c == quote => {
                    out.push('\\');
                    out.push(c);
                }
                c if !printable(c) => {
                    let n = c as u32;
                    out.push_str(&if n <= 255 {
                        format!("\\x{n:02x}")
                    } else if n <= 65535 {
                        format!("\\u{n:04x}")
                    } else {
                        format!("\\U{n:08x}")
                    });
                }
                c => out.push(c),
            }
        }
        out.push(quote);
        out
    } else {
        py_str(v)
    }
}
fn printable(c: char) -> bool {
    static RANGES: OnceLock<Vec<(u32, u32)>> = OnceLock::new();
    let ranges = RANGES.get_or_init(|| {
        include_str!("unicode_nonprintable.tsv")
            .lines()
            .map(|l| {
                let (a, b) = l.split_once('\t').unwrap();
                (
                    u32::from_str_radix(a, 16).unwrap(),
                    u32::from_str_radix(b, 16).unwrap(),
                )
            })
            .collect()
    });
    let n = c as u32;
    let i = ranges.partition_point(|(a, _)| *a <= n);
    i == 0 || n > ranges[i - 1].1
}

pub fn human(v: &Document) -> String {
    match v {
        Document::Bool(b) => if *b { "yes" } else { "no" }.into(),
        Document::Null => "unknown".into(),
        Document::String(s) if s.is_empty() => "unknown".into(),
        _ => py_str(v),
    }
}
pub fn money(v: Option<f64>) -> crate::Result<String> {
    let Some(v) = v else {
        return Ok("unknown".into());
    };
    if !v.is_finite() {
        return Err(crate::Failure::error(
            "cannot round non-finite copper amount",
        ));
    }
    let n = format!("{:.0}", v.round_ties_even().abs());
    let padded = format!("{n:0>5}");
    let split = padded.len() - 4;
    let g = padded[..split].trim_start_matches('0');
    let s: u8 = padded[split..split + 2].parse().unwrap();
    let c: u8 = padded[split + 2..].parse().unwrap();
    let mut parts = vec![];
    if !g.is_empty() {
        parts.push(format!("{g}g"));
    }
    if s != 0 {
        parts.push(format!("{s}s"));
    }
    if c != 0 {
        parts.push(format!("{c}c"));
    }
    Ok(format!(
        "{}{}",
        if v < 0.0 { "-" } else { "" },
        if parts.is_empty() {
            "0c".into()
        } else {
            parts.join(" ")
        }
    ))
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
type Mappings = HashMap<char, (String, String)>;
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
                    (decode(c[1]), decode(c[2])),
                )
            })
            .collect()
    })
}
#[derive(Clone, Copy, Default)]
struct Properties {
    cased: bool,
    ignorable: bool,
    decimal: Option<u32>,
    digit: Option<u32>,
}
fn properties(c: char) -> Properties {
    static MAP: OnceLock<HashMap<char, Properties>> = OnceLock::new();
    *MAP.get_or_init(|| {
        include_str!("unicode_properties.tsv")
            .lines()
            .map(|l| {
                let v = l.split('\t').collect::<Vec<_>>();
                (
                    char::from_u32(u32::from_str_radix(v[0], 16).unwrap()).unwrap(),
                    Properties {
                        cased: v[1] == "1",
                        ignorable: v[2] == "1",
                        decimal: v[3].parse().ok(),
                        digit: v[4].parse().ok(),
                    },
                )
            })
            .collect()
    })
    .get(&c)
    .unwrap_or(&Properties::default())
}
pub fn casefold(s: &str) -> String {
    s.chars()
        .map(|c| {
            mappings()
                .get(&c)
                .map(|v| v.0.clone())
                .unwrap_or_else(|| c.to_string())
        })
        .collect()
}
pub fn lower(s: &str) -> String {
    let chars: Vec<_> = s.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == 'Σ'
            && chars[..i]
                .iter()
                .rev()
                .find(|&&c| !properties(c).ignorable)
                .is_some_and(|&c| properties(c).cased)
            && !chars[i + 1..]
                .iter()
                .find(|&&c| !properties(c).ignorable)
                .is_some_and(|&c| properties(c).cased)
        {
            out.push('ς');
        } else {
            out.push_str(
                &mappings()
                    .get(&c)
                    .map(|v| v.1.clone())
                    .unwrap_or_else(|| c.to_string()),
            );
        }
    }
    out
}
pub fn decimal(s: &str) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    s.chars()
        .map(|c| {
            properties(c)
                .decimal
                .map(|d| char::from_digit(d, 10).unwrap())
        })
        .collect()
}
pub fn isdigit(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| properties(c).digit.is_some())
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
pub fn urlencode(params: &[(String, Document)]) -> String {
    params
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                quote(k).replace("%20", "+"),
                quote(&py_str(v)).replace("%20", "+")
            )
        })
        .collect::<Vec<_>>()
        .join("&")
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
pub fn base64_decode(s: &str) -> crate::Result<Vec<u8>> {
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
            return Err(crate::Failure::usage("invalid snapshot page cursor"));
        }
        let b = match b {
            b'+' => b'-',
            b'/' => b'_',
            b => b,
        };
        let n = B64
            .iter()
            .position(|&v| v == b)
            .ok_or_else(|| crate::Failure::usage("invalid snapshot page cursor"))?;
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
        return Err(crate::Failure::usage("invalid snapshot page cursor"));
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
pub fn number(n: f64) -> Document {
    Document::Number(Number::Float(n))
}
