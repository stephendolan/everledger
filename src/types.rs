use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Copper(pub i64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EpochSeconds(pub i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Answer = 0,
    Error = 1,
    Usage = 2,
    Stale = 3,
    Unreachable = 4,
}

#[derive(Clone, Debug)]
pub struct Failure {
    pub message: String,
    pub code: ExitCode,
    pub body: Option<crate::document::Document>,
}
impl Failure {
    pub fn new(message: impl Into<String>, code: ExitCode) -> Self {
        Self {
            message: message.into(),
            code,
            body: None,
        }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message, ExitCode::Error)
    }
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(message, ExitCode::Usage)
    }
    pub fn unreachable(message: impl Into<String>) -> Self {
        Self::new(message, ExitCode::Unreachable)
    }
    pub fn shape(boundary: &str, error: impl fmt::Display) -> Self {
        Self::error(format!("{boundary}: invalid response shape ({error})"))
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Failure {}
impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Self::error(format!("error: {e}"))
    }
}
pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliInteger(String);
impl std::str::FromStr for CliInteger {
    type Err = String;
    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let value =
            value.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c));
        let (negative, value) = if let Some(v) = value.strip_prefix('-') {
            (true, v)
        } else {
            (false, value.strip_prefix('+').unwrap_or(value))
        };
        if value.is_empty()
            || value.starts_with('_')
            || value.ends_with('_')
            || value.contains("__")
        {
            return Err("invalid integer".into());
        }
        let digits = crate::primitives::decimal(&value.replace('_', ""))
            .ok_or_else(|| "invalid integer".to_string())?;
        let digits = digits.trim_start_matches('0');
        let text = if digits.is_empty() {
            "0".into()
        } else {
            format!("{}{digits}", if negative { "-" } else { "" })
        };
        Ok(Self(text))
    }
}
impl From<i64> for CliInteger {
    fn from(value: i64) -> Self {
        Self(value.to_string())
    }
}
impl fmt::Display for CliInteger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl PartialOrd for CliInteger {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for CliInteger {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let a = self.0.strip_prefix('-');
        let b = other.0.strip_prefix('-');
        match (a, b) {
            (Some(a), Some(b)) => b.len().cmp(&a.len()).then_with(|| b.cmp(a)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => self
                .0
                .len()
                .cmp(&other.0.len())
                .then_with(|| self.0.cmp(&other.0)),
        }
    }
}
impl CliInteger {
    pub fn as_i64(&self) -> Option<i64> {
        self.0.parse().ok()
    }
    pub fn document(&self) -> crate::document::Document {
        crate::document::Document::Number(crate::document::Number::Integer(self.0.clone()))
    }
    pub fn is_negative(&self) -> bool {
        self.0.starts_with('-')
    }
}
