use crate::{ExitCode, Result, clock::Clock, document::Document, http::Payload};
use serde::{Deserialize, Deserializer};

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Field<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(d)? {
            Some(v) => Self::Value(v),
            None => Self::Null,
        })
    }
}
impl<T> Field<T> {
    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Value(v) => Some(v),
            Self::Missing | Self::Null => None,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum SourceAge {
    Integer(i64),
    Float(f64),
}
impl<'de> Deserialize<'de> for SourceAge {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let n = serde_json::Number::deserialize(d)?;
        if let Some(n) = n.as_i64() {
            Ok(Self::Integer(n))
        } else {
            n.as_f64()
                .map(Self::Float)
                .ok_or_else(|| serde::de::Error::custom("age out of range"))
        }
    }
}
impl SourceAge {
    pub fn seconds(self) -> f64 {
        match self {
            Self::Integer(n) => n as f64,
            Self::Float(n) => n,
        }
    }
    pub fn text(self) -> String {
        match self {
            Self::Integer(n) => n.to_string(),
            Self::Float(n) => crate::primitives::float_repr(n),
        }
    }
}
impl PartialEq for SourceAge {
    fn eq(&self, other: &Self) -> bool {
        self.seconds() == other.seconds()
    }
}
#[derive(Clone, Debug, Deserialize, Default)]
pub struct SourceData {
    pub name: Option<String>,
    pub file: Option<String>,
    pub path: Option<String>,
    pub saved_at: Option<String>,
    pub scanned_at: Option<String>,
    pub mtime: Option<String>,
    pub updated_at: Option<String>,
    pub fetched_at: Option<String>,
    #[serde(default)]
    pub scan_age_seconds: Field<SourceAge>,
    #[serde(default)]
    pub file_age_seconds: Field<SourceAge>,
    #[serde(default)]
    pub age_seconds: Field<SourceAge>,
    pub stale: Option<bool>,
    pub detail: Option<String>,
}
pub type Source = Payload<SourceData>;
pub fn source(name: &str, timestamp: Option<&str>, clock: &dyn Clock) -> Result<Source> {
    let age = crate::time::age(timestamp, clock);
    let doc = Document::object([
        ("name", Document::from(name)),
        ("updated_at", Document::from(timestamp)),
        ("age_seconds", Document::from(age)),
    ]);
    Payload::decode(doc, "source")
}
pub fn sources(data: &Document) -> Result<Vec<Source>> {
    match data.get("sources") {
        None => Ok(vec![]),
        Some(Document::Array(a)) => a
            .iter()
            .map(|s| Payload::decode(s.clone(), "source"))
            .collect(),
        _ => Err(crate::Failure::shape("sources", "expected array")),
    }
}
pub fn is_stale(items: &[Source]) -> bool {
    items.iter().any(|s| s.view().stale == Some(true))
}
pub fn stale_code(items: &[Source]) -> ExitCode {
    if is_stale(items) {
        ExitCode::Stale
    } else {
        ExitCode::Answer
    }
}
