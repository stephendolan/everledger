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

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Venue {
    Auction,
    Vendor,
}
