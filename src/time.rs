use crate::{
    EpochSeconds, Failure, Result,
    clock::Clock,
    document::{Document, Number},
};
use jiff::{civil::Date, tz::TimeZone};

const MIN_MICROS: i64 = -62_135_596_800_000_000;
const MAX_MICROS: i64 = 253_402_300_799_999_999;
const EPOCH: jiff::civil::DateTime = jiff::civil::date(1970, 1, 1).at(0, 0, 0, 0);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcInstant {
    micros: i64,
}
impl UtcInstant {
    pub const UNIX_EPOCH: Self = Self { micros: 0 };
    pub fn from_microseconds(micros: i64) -> Result<Self> {
        if (MIN_MICROS..=MAX_MICROS).contains(&micros) {
            Ok(Self { micros })
        } else {
            Err(Failure::error("instant outside Python datetime range"))
        }
    }
    pub fn from_epoch(value: EpochSeconds) -> Result<Self> {
        Self::from_microseconds(
            value
                .0
                .checked_mul(1_000_000)
                .ok_or_else(|| Failure::error("time overflow"))?,
        )
    }
    pub fn microseconds(self) -> i64 {
        self.micros
    }
    pub fn add_microseconds(self, us: i64) -> Result<Self> {
        Self::from_microseconds(
            self.micros
                .checked_add(us)
                .ok_or_else(|| Failure::error("time overflow"))?,
        )
    }
}
pub fn iso(value: UtcInstant) -> String {
    let dt = EPOCH.saturating_add(jiff::SignedDuration::from_micros(
        value.micros.div_euclid(1_000_000) * 1_000_000,
    ));
    format!("{dt:.0}Z")
}
pub fn instant(value: &str) -> Result<UtcInstant> {
    instant_in(value, None)
}
pub fn instant_in(value: &str, zone: Option<&str>) -> Result<UtcInstant> {
    fn invalid() -> Failure {
        Failure::error("invalid ISO timestamp")
    }
    fn n(s: &str) -> Result<i32> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        s.parse().map_err(|_| invalid())
    }
    fn clock(s: &str) -> Result<(i32, i32, i32, i32)> {
        let (base, fraction) = s
            .split_once(['.', ','])
            .map_or((s, None), |(a, b)| (a, Some(b)));
        let parts = if base.contains(':') {
            base.split(':').collect::<Vec<_>>()
        } else {
            if !base.is_ascii() || !base.len().is_multiple_of(2) {
                return Err(invalid());
            }
            base.as_bytes()
                .chunks(2)
                .map(|b| std::str::from_utf8(b).unwrap())
                .collect()
        };
        if parts.is_empty() || parts.len() > 3 || parts.iter().any(|s| s.len() != 2) {
            return Err(invalid());
        }
        let mut v = [0; 3];
        for (i, p) in parts.iter().enumerate() {
            v[i] = n(p)?;
        }
        let us = if let Some(f) = fraction {
            if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            let f = &f[..f.len().min(6)];
            n(f)? * 10i32.pow(6 - f.len() as u32)
        } else {
            0
        };
        Ok((v[0], v[1], v[2], us))
    }
    let s = value.replace('Z', "+00:00");
    if !s.is_char_boundary(4) {
        return Err(invalid());
    }
    let year = n(s.get(..4).ok_or_else(invalid)?)?;
    if !(1..=9999).contains(&year) {
        return Err(invalid());
    }
    let extended = s.as_bytes().get(4) == Some(&b'-');
    let start = if extended { 5 } else { 4 };
    let (date, end) = if s.as_bytes().get(start) == Some(&b'W') {
        let week = n(s.get(start + 1..start + 3).ok_or_else(invalid)?)?;
        let mut end = start + 3;
        let mut day = 1;
        if extended && s.as_bytes().get(end) == Some(&b'-') {
            day = n(s.get(end + 1..end + 2).ok_or_else(invalid)?)?;
            end += 2;
        } else if !extended && s.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
            day = n(s.get(end..end + 1).ok_or_else(invalid)?)?;
            end += 1;
        }
        if !(1..=53).contains(&week) || !(1..=7).contains(&day) {
            return Err(invalid());
        }
        let jan = Date::new(year as i16, 1, 4).map_err(|_| invalid())?;
        let offset = (week - 1) * 7 + day - 1 - (jan.weekday().to_monday_zero_offset() as i32);
        let date = jan
            .checked_add(jiff::Span::new().days(offset))
            .map_err(|_| invalid())?;
        if week == 53 && date.iso_week_date().year() != year as i16 {
            return Err(invalid());
        }
        (date, end)
    } else {
        let month = n(s.get(start..start + 2).ok_or_else(invalid)?)?;
        let dstart = if extended {
            if s.as_bytes().get(7) != Some(&b'-') {
                return Err(invalid());
            }
            8
        } else {
            6
        };
        let day = n(s.get(dstart..dstart + 2).ok_or_else(invalid)?)?;
        (
            Date::new(year as i16, month as i8, day as i8).map_err(|_| invalid())?,
            dstart + 2,
        )
    };
    let mut dt = date.at(0, 0, 0, 0);
    let mut offset = None;
    if s.len() > end {
        let sep = s[end..].chars().next().ok_or_else(invalid)?;
        let tail = &s[end + sep.len_utf8()..];
        let split = tail.find(['+', '-']);
        let (tm, tz) = split.map_or((tail, None), |i| (&tail[..i], Some(&tail[i..])));
        let (h, m, sec, us) = clock(tm)?;
        if h > 23 || m > 59 || sec > 59 {
            return Err(invalid());
        }
        dt = date.at(h as i8, m as i8, sec as i8, us * 1000);
        if let Some(tz) = tz {
            let (h, m, s, us) = clock(&tz[1..])?;
            let seconds = h * 3600 + m * 60 + s;
            if seconds >= 86400 {
                return Err(invalid());
            }
            offset = Some(
                (if tz.starts_with('-') { -1 } else { 1 })
                    * (i64::from(seconds) * 1_000_000 + i64::from(us)),
            );
        }
    }
    let offset = match offset {
        Some(us) => us,
        None => {
            let tz = match zone {
                Some(z) => TimeZone::get(z).map_err(|_| invalid())?,
                None => TimeZone::system(),
            };
            let offset = match tz.to_ambiguous_timestamp(dt).offset() {
                jiff::tz::AmbiguousOffset::Unambiguous { offset } => offset,
                jiff::tz::AmbiguousOffset::Gap { before, after: _ }
                | jiff::tz::AmbiguousOffset::Fold { before, after: _ } => before,
            };
            i64::from(offset.seconds()) * 1_000_000
        }
    };
    let micros = i64::try_from(dt.duration_since(EPOCH).as_micros()).map_err(|_| invalid())?;
    UtcInstant::from_microseconds(micros - offset)
}
pub fn time_selector(value: &str, clock: &dyn Clock) -> Result<String> {
    time_selector_in(value, clock, None)
}
pub fn time_selector_in(value: &str, clock: &dyn Clock, zone: Option<&str>) -> Result<String> {
    let err = || Failure::usage("time must be 7d, 24h, a date, or an ISO timestamp");
    let suffix = value.chars().last();
    if matches!(suffix, Some('d' | 'h'))
        && let Some(n) = decimal_seconds(&value[..value.len() - 1])
    {
        let scale = if suffix == Some('d') {
            86_400_000_000i64
        } else {
            3_600_000_000
        };
        return Ok(iso(clock
            .now()
            .add_microseconds(
                n.checked_mul(scale)
                    .and_then(i64::checked_neg)
                    .ok_or_else(err)?,
            )
            .map_err(|_| err())?));
    }
    instant_in(value, zone).map(iso).map_err(|_| err())
}
pub fn roster_seconds(value: &Document) -> Result<EpochSeconds> {
    roster_seconds_in(value, None)
}
pub fn roster_seconds_in(value: &Document, zone: Option<&str>) -> Result<EpochSeconds> {
    match value {
        Document::Bool(b) => Ok(EpochSeconds(i64::from(*b))),
        Document::Number(Number::Integer(s)) => s
            .parse()
            .map(EpochSeconds)
            .map_err(|_| Failure::error("epoch seconds out of range")),
        Document::String(s) => {
            if let Some(n) = decimal_seconds(s) {
                Ok(EpochSeconds(n))
            } else {
                instant_in(s, zone).map(|t| EpochSeconds(t.microseconds() / 1_000_000))
            }
        }
        _ => Err(Failure::error("invalid epoch seconds")),
    }
}
pub fn age(value: Option<&str>, clock: &dyn Clock) -> Option<i64> {
    age_in(value, clock, None)
}
pub fn age_in(value: Option<&str>, clock: &dyn Clock, zone: Option<&str>) -> Option<i64> {
    let t = instant_in(value?, zone).ok()?;
    Some(((clock.now().microseconds() - t.microseconds()) / 1_000_000).max(0))
}

fn decimal_seconds(value: &str) -> Option<i64> {
    const ZEROES: &[u32] = &[
        0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
        0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
        0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0,
        0xff10, 0x104a0, 0x10d30, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450, 0x114d0,
        0x11650, 0x116c0, 0x11730, 0x118e0, 0x11950, 0x11c50, 0x11d50, 0x11da0, 0x11f50, 0x16a60,
        0x16ac0, 0x16b50, 0x1d7ce, 0x1d7d8, 0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e4f0,
        0x1e950, 0x1fbf0,
    ];
    if value.is_empty() {
        return None;
    }
    value.chars().try_fold(0i64, |n, c| {
        let cp = c as u32;
        let zero = ZEROES.iter().find(|&&z| cp >= z && cp - z < 10)?;
        n.checked_mul(10)?.checked_add(i64::from(cp - zero))
    })
}
