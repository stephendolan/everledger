use crate::{
    Copper, Failure, Result, catalogue::nullable, document::Document, http::Payload,
    primitives::casefold,
};
use serde::{Deserialize, Deserializer, de::Error};

pub const PAGE_SIZE: usize = 100;
pub fn invalid() -> Failure {
    Failure::unreachable("ForeverDB API shape mismatch; check vendor mapping before syncing")
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "i64")]
pub struct Identifier(pub i64);
impl TryFrom<i64> for Identifier {
    type Error = &'static str;
    fn try_from(n: i64) -> std::result::Result<Self, Self::Error> {
        if n > 0 {
            Ok(Self(n))
        } else {
            Err("expected positive identifier")
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "String")]
struct Text(String);
impl TryFrom<String> for Text {
    type Error = &'static str;
    fn try_from(s: String) -> std::result::Result<Self, Self::Error> {
        if s.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
            .is_empty()
        {
            Err("expected nonblank text")
        } else {
            Ok(Self(s))
        }
    }
}
fn copper<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<Copper>, D::Error> {
    let n = Option::<Copper>::deserialize(d)?;
    if n.is_some_and(|n| n.0 < 0) {
        Err(D::Error::custom("negative copper"))
    } else {
        Ok(n)
    }
}
fn stock<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<i64>, D::Error> {
    let n = Option::<i64>::deserialize(d)?;
    if n.is_some_and(|n| n < 0) {
        Err(D::Error::custom("negative stock"))
    } else {
        Ok(n)
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Coordinate {
    Integer(i64),
    Float(f64),
}
impl Coordinate {
    fn valid(&self) -> bool {
        let n = match self {
            Self::Integer(n) => *n as f64,
            Self::Float(n) => *n,
        };
        (0.0..=100.0).contains(&n)
    }
}
fn coordinate<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Coordinate>, D::Error> {
    let n = Option::<Coordinate>::deserialize(d)?;
    if n.as_ref().is_some_and(|n| !n.valid()) {
        Err(D::Error::custom("coordinate out of range"))
    } else {
        Ok(n)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Basis {
    Seen,
    Baseline,
    Unverified,
}
impl Basis {
    pub fn name(self) -> &'static str {
        match self {
            Self::Seen => "seen",
            Self::Baseline => "baseline",
            Self::Unverified => "unverified",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Seen => "seen in Forever",
            Self::Baseline => "Classic-only baseline",
            Self::Unverified => "unverified",
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
struct Claims {
    #[serde(rename = "seenInForever")]
    seen: Option<bool>,
    #[serde(rename = "classicData")]
    classic: Option<bool>,
}
impl Claims {
    fn basis(&self) -> Basis {
        if self.seen == Some(true) {
            Basis::Seen
        } else if self.classic == Some(true) {
            Basis::Baseline
        } else {
            Basis::Unverified
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
struct Sale {
    #[serde(rename = "itemId")]
    item: Identifier,
    #[serde(deserialize_with = "copper")]
    price: Option<Copper>,
    stack: Identifier,
    limited: bool,
    #[serde(default, rename = "limitedStockCount", deserialize_with = "stock")]
    stock: Option<i64>,
    name: Option<String>,
    #[serde(flatten)]
    claims: Claims,
}
#[derive(Clone, Debug, Deserialize)]
struct Vendor {
    id: Identifier,
    name: Text,
    zone: Text,
    #[serde(default)]
    title: crate::provenance::Field<crate::primitives::Scalar>,
    #[serde(default, rename = "mapId")]
    _map_id: Option<serde::de::IgnoredAny>,
    #[serde(default, deserialize_with = "coordinate")]
    x: Option<Coordinate>,
    #[serde(default, deserialize_with = "coordinate")]
    y: Option<Coordinate>,
    side: Option<crate::primitives::Scalar>,
    sells: Vec<Sale>,
    #[serde(flatten)]
    claims: Claims,
}
#[derive(Deserialize)]
struct Envelope {
    data: Vendor,
}
#[derive(Deserialize)]
struct PageRow {
    id: Identifier,
}
#[derive(Deserialize)]
struct PageMeta {
    total: usize,
    limit: usize,
    offset: usize,
    build: Text,
}
#[derive(Deserialize)]
struct Page {
    data: Vec<PageRow>,
    meta: PageMeta,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Metadata {
    pub build: String,
    pub vendor_total: usize,
    #[serde(deserialize_with = "nullable")]
    pub published_at: Option<String>,
}
impl Metadata {
    pub fn document(&self) -> Document {
        Document::object([
            ("build", self.build.clone().into()),
            ("vendor_total", Document::from(self.vendor_total)),
            ("published_at", self.published_at.as_deref().into()),
        ])
    }
}
pub struct ListPage {
    pub ids: Vec<String>,
    pub next: Option<usize>,
    pub metadata: Metadata,
}
pub fn page(doc: Document, offset: usize) -> Result<ListPage> {
    let p: Page = doc.decode("ForeverDB page").map_err(|_| invalid())?;
    if p.meta.limit != PAGE_SIZE
        || p.meta.offset != offset
        || p.data.len() != PAGE_SIZE.min(p.meta.total.saturating_sub(offset))
    {
        return Err(invalid());
    }
    let mut ids = vec![];
    for row in p.data {
        let id = row.id.0.to_string();
        if ids.contains(&id) {
            return Err(invalid());
        }
        ids.push(id);
    }
    let next = if offset.saturating_add(ids.len()) < p.meta.total {
        Some(offset.checked_add(PAGE_SIZE).ok_or_else(invalid)?)
    } else {
        None
    };
    Ok(ListPage {
        ids,
        next,
        metadata: Metadata {
            build: p.meta.build.0,
            vendor_total: p.meta.total,
            published_at: None,
        },
    })
}
fn coord(n: &Option<Coordinate>) -> Document {
    match n {
        None => Document::Null,
        Some(Coordinate::Integer(n)) => (*n).into(),
        Some(Coordinate::Float(n)) => (*n).into(),
    }
}
pub fn vendor(doc: Document, expected: Identifier) -> Result<Document> {
    let payload: Payload<Envelope> =
        Payload::decode(doc, "ForeverDB vendor").map_err(|_| invalid())?;
    let v = &payload.view().data;
    if v.id != expected {
        return Err(invalid());
    }
    let raw_sales = payload
        .document()
        .get("data")
        .and_then(|d| d.get("sells"))
        .and_then(Document::as_array)
        .ok_or_else(invalid)?;
    let zone_slug = casefold(&v.zone.0).replace(' ', "-");
    let mut rows = vec![];
    for (s, raw) in v.sells.iter().zip(raw_sales) {
        let claim = s.claims.basis();
        let per = s
            .price
            .filter(|p| p.0 % s.stack.0 == 0)
            .map(|p| Copper(p.0 / s.stack.0));
        let title = match &v.title {
            crate::provenance::Field::Missing => "".into(),
            crate::provenance::Field::Null => Document::Null,
            crate::provenance::Field::Value(d) => scalar(Some(d)),
        };
        let row = Document::object([
            ("npc_id", expected.0.into()),
            ("name", v.name.0.clone().into()),
            ("title", title),
            ("zone", v.zone.0.clone().into()),
            ("zone_slug", zone_slug.clone().into()),
            (
                "mapId",
                payload
                    .document()
                    .get("data")
                    .and_then(|d| d.get("mapId"))
                    .cloned()
                    .unwrap_or(Document::Null),
            ),
            ("side", scalar(v.side.as_ref())),
            (
                "coords",
                Document::Array(if v.x.is_some() && v.y.is_some() {
                    vec![Document::Array(vec![coord(&v.x), coord(&v.y)])]
                } else {
                    vec![]
                }),
            ),
            ("price_copper", per.map(|n| n.0).into()),
            ("stack_price_copper", s.price.map(|n| n.0).into()),
            ("stack", s.stack.0.into()),
            ("limited", s.limited.into()),
            ("limitedStockCount", s.stock.into()),
            ("status_raw", s.claims.seen.into()),
            ("basis", claim.name().into()),
            ("vendor_basis", v.claims.basis().name().into()),
            ("sale_basis", claim.label().into()),
            (
                "raw",
                Document::object([("vendor_seen", v.claims.seen.into()), ("sell", raw.clone())]),
            ),
        ]);
        rows.push(Document::object([
            (
                "item",
                Document::object([("id", s.item.0.into()), ("name", s.name.as_deref().into())]),
            ),
            ("row", row),
        ]));
    }
    Ok(Document::object([("sells", Document::Array(rows))]))
}

fn scalar(value: Option<&crate::primitives::Scalar>) -> Document {
    use crate::primitives::Scalar;
    match value {
        None => Document::Null,
        Some(Scalar::Bool(n)) => (*n).into(),
        Some(Scalar::Integer(n)) => (*n).into(),
        Some(Scalar::Float(n)) => (*n).into(),
        Some(Scalar::Text(n)) => n.clone().into(),
    }
}
