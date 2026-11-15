use crate::{
    Context, Copper, Failure, Result,
    catalogue::{Item, OrderedMap, nullable},
    document::{Document, JsonMode},
    http::{Payload, Request},
    primitives::Scalar,
    provenance::{Field, Source},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const BACKEND: &str = "wowforever.ninja";
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Unchanged,
    Changed,
    New,
    Baseline,
    Unverified,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Basis {
    Seen,
    Baseline,
    Unverified,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Seller {
    pub npc_id: i64,
    pub name: String,
    pub title: String,
    pub zone: String,
    pub zone_slug: String,
    pub coords: Vec<Vec<Option<Scalar>>>,
    #[serde(deserialize_with = "nullable")]
    pub price_copper: Option<Copper>,
    #[serde(deserialize_with = "nullable")]
    pub price_raw: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub status_raw: Option<String>,
    pub raw: Vendor,
    pub status: Status,
    pub basis: Basis,
    pub sale_basis: String,
    pub price_basis: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Vendor {
    pub n: String,
    #[serde(default)]
    pub s: String,
    pub f: Option<String>,
}
#[derive(Debug, Deserialize)]
struct VendorLayer<T> {
    #[serde(default)]
    vendor: Vec<T>,
}
impl<T> Default for VendorLayer<T> {
    fn default() -> Self {
        Self { vendor: vec![] }
    }
}
#[derive(Debug, Deserialize)]
struct ZoneName {
    #[serde(default)]
    name: Field<String>,
}
#[derive(Debug, Deserialize)]
struct Zone {
    #[serde(default)]
    zone: Field<ZoneName>,
    #[serde(default)]
    pins: VendorLayer<Vec<Option<Scalar>>>,
    #[serde(default)]
    codes: VendorLayer<Option<String>>,
    #[serde(default = "empty_map")]
    vendors: OrderedMap<Vendor>,
}
fn empty_map<T>() -> OrderedMap<T> {
    OrderedMap(vec![])
}
#[derive(Debug, Deserialize)]
struct Pins {
    #[serde(default = "empty_map")]
    zones: OrderedMap<Zone>,
}
#[derive(Debug, Deserialize)]
struct NinjaItem {
    name: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Snapshot {
    pub item: Item,
    #[serde(deserialize_with = "nullable")]
    pub vendors: Option<Vec<Seller>>,
    #[serde(deserialize_with = "nullable")]
    pub reason: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub published_at: Option<String>,
    pub fetched_at: String,
    pub backend: String,
    pub backend_url: Option<String>,
    pub stale: Option<bool>,
    pub known_zones: Option<Vec<String>>,
}
#[derive(Clone, Debug)]
pub enum Coverage {
    ItemAbsent,
    PinsAbsent,
    Sellers(Vec<Seller>),
}
impl Coverage {
    pub fn rows(&self) -> &[Seller] {
        match self {
            Self::ItemAbsent | Self::PinsAbsent => &[],
            Self::Sellers(rows) => rows,
        }
    }
}
#[derive(Clone, Debug)]
pub struct NinjaAnswer {
    payload: Payload<Snapshot>,
    coverage: Coverage,
}
impl NinjaAnswer {
    pub fn payload(&self) -> &Payload<Snapshot> {
        &self.payload
    }
    pub fn coverage(&self) -> &Coverage {
        &self.coverage
    }
    fn decode(doc: Document) -> Result<Self> {
        let payload: Payload<Snapshot> = Payload::decode(doc, "ninja snapshot")?;
        let coverage = match (&payload.view().vendors, payload.view().reason.as_deref()) {
            (None, Some("item not in dataset")) => Coverage::ItemAbsent,
            (None, Some("no pins published")) => Coverage::PinsAbsent,
            (Some(rows), None) => Coverage::Sellers(rows.clone()),
            _ => {
                return Err(Failure::shape(
                    "ninja snapshot",
                    "inconsistent vendor coverage",
                ));
            }
        };
        Ok(Self { payload, coverage })
    }
    pub fn select_zone(&mut self, query: &str) -> Result<()> {
        let zones = self
            .coverage
            .rows()
            .iter()
            .map(|r| r.zone.clone())
            .collect::<BTreeSet<_>>();
        let query = crate::primitives::casefold(query);
        let q = match query.as_str() {
            "if" => "ironforge",
            "sw" => "stormwind",
            "darn" => "darnassus",
            "org" => "orgrimmar",
            "tb" => "thunder bluff",
            "uc" => "undercity",
            _ => &query,
        };
        let docs = self
            .payload
            .document()
            .get("vendors")
            .and_then(Document::as_array);
        if let Coverage::Sellers(rows) = &self.coverage {
            let docs =
                docs.ok_or_else(|| Failure::shape("ninja snapshot", "missing seller documents"))?;
            let filtered = rows
                .iter()
                .zip(docs)
                .filter(|(r, _)| {
                    crate::primitives::casefold(&r.zone).contains(q)
                        || crate::primitives::casefold(&r.zone_slug).contains(q)
                })
                .map(|(_, d)| d.clone())
                .collect();
            self.payload.set("vendors", Document::Array(filtered))?;
            self.coverage = Coverage::Sellers(
                self.payload
                    .view()
                    .vendors
                    .clone()
                    .ok_or_else(|| Failure::shape("ninja snapshot", "missing sellers"))?,
            );
        }
        self.payload
            .set("known_zones", Document::from_typed(&zones)?)?;
        Ok(())
    }
    pub fn public(&self) -> Document {
        let mut d = self.payload.document().clone();
        for key in ["raw_item", "raw_pins", "backend_url", "_fallback_vendors"] {
            d.remove(key);
        }
        d
    }
    pub fn sellers_document(&self) -> Result<Document> {
        self.payload
            .document()
            .get("vendors")
            .cloned()
            .ok_or_else(|| Failure::shape("ninja snapshot", "missing vendors"))
    }
    pub fn reason(&self) -> Option<&'static str> {
        match self.coverage {
            Coverage::ItemAbsent => Some("item not in dataset"),
            Coverage::PinsAbsent => Some("no pins published"),
            Coverage::Sellers(_) => None,
        }
    }
}
#[derive(Clone, Debug)]
pub enum VendorAnswer {
    Ninja(NinjaAnswer),
}
pub fn lookup(item_id: i64, refresh: bool, context: &Context<'_>) -> Result<VendorAnswer> {
    Ok(VendorAnswer::Ninja(ninja_lookup(
        item_id, refresh, context,
    )?))
}
const DECIMAL_ZEROES: &[u32] = &[
    0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
    0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
    0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0, 0xff10,
    0x104a0, 0x10d30, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450, 0x114d0, 0x11650,
    0x116c0, 0x11730, 0x118e0, 0x11950, 0x11c50, 0x11d50, 0x11da0, 0x11f50, 0x16a60, 0x16ac0,
    0x16b50, 0x1d7ce, 0x1d7d8, 0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e4f0, 0x1e950,
    0x1fbf0,
];
fn decimal(c: char) -> Option<i64> {
    let code = u32::from(c);
    DECIMAL_ZEROES
        .iter()
        .find_map(|zero| code.checked_sub(*zero).filter(|n| *n < 10).map(i64::from))
}
fn price_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}
pub fn parse_price(text: &str) -> Option<Copper> {
    let mut rest = text.trim_matches(price_space);
    let mut total = 0_i64;
    let mut units = BTreeSet::new();
    let mut count = 0;
    while !rest.is_empty() {
        let mut end = 0;
        let mut number = 0_i64;
        for (offset, c) in rest.char_indices() {
            let Some(digit) = decimal(c) else {
                break;
            };
            number = number.checked_mul(10)?.checked_add(digit)?;
            end = offset + c.len_utf8();
        }
        if end == 0 {
            return None;
        }
        rest = rest.get(end..)?;
        let unit = rest.chars().next()?;
        let value = match unit {
            'g' => 10000,
            's' => 100,
            'c' => 1,
            _ => return None,
        };
        if !units.insert(unit) {
            return None;
        }
        total = total.checked_add(number.checked_mul(value)?)?;
        rest = rest.get(unit.len_utf8()..)?.trim_start_matches(price_space);
        count += 1;
    }
    (count > 0).then_some(Copper(total))
}

pub fn normalize(document: Document) -> Result<Vec<Payload<Seller>>> {
    let pins: Payload<Pins> = Payload::decode(document, "ninja pins")?;
    let mut rows = vec![];
    for (slug, zone) in &pins.view().zones.0 {
        let mut locations: BTreeMap<String, Vec<Vec<Option<Scalar>>>> = BTreeMap::new();
        let mut codes: BTreeMap<String, Option<String>> = BTreeMap::new();
        for (i, pin) in zone.pins.vendor.iter().enumerate() {
            let [x, y, npc, ..] = pin.as_slice() else {
                return Err(Failure::shape("ninja pin", "expected x, y, npc"));
            };
            let id = npc
                .as_ref()
                .map(Scalar::python_str)
                .unwrap_or_else(|| "None".into());
            locations
                .entry(id.clone())
                .or_default()
                .push(vec![x.clone(), y.clone()]);
            if let Some(code) = zone.codes.vendor.get(i) {
                codes.entry(id).or_insert_with(|| code.clone());
            }
        }
        for (npc, v) in &zone.vendors.0 {
            let npc_id = npc
                .parse::<i64>()
                .map_err(|e| Failure::shape("ninja vendor id", e))?;
            let raw = pins
                .document()
                .get("zones")
                .and_then(|d| d.get(slug))
                .and_then(|d| d.get("vendors"))
                .and_then(|d| d.get(npc))
                .cloned()
                .ok_or_else(|| Failure::shape("ninja vendor", "missing document"))?;
            let (title, price) = match v.s.rsplit_once(" · ") {
                Some((t, p)) => (t, Some(p)),
                None => (v.s.as_str(), None),
            };
            let code = codes.get(npc).cloned().unwrap_or_else(|| v.f.clone());
            let (status, basis) = match code.as_deref() {
                Some("u") => (Status::Unchanged, Basis::Seen),
                Some("c") => (Status::Changed, Basis::Seen),
                Some("n") => (Status::New, Basis::Seen),
                None => (Status::Baseline, Basis::Baseline),
                Some(_) => (Status::Unverified, Basis::Unverified),
            };
            let d = Document::object([
                ("npc_id", Document::from(npc_id)),
                ("name", Document::from(v.n.clone())),
                ("title", Document::from(title)),
                (
                    "zone",
                    Document::from(match &zone.zone {
                        Field::Missing => slug.as_str(),
                        Field::Null => return Err(Failure::shape("ninja zone", "zone is null")),
                        Field::Value(z) => match &z.name {
                            Field::Missing => slug.as_str(),
                            Field::Null => {
                                return Err(Failure::shape("ninja zone", "zone name is null"));
                            }
                            Field::Value(name) => name.as_str(),
                        },
                    }),
                ),
                ("zone_slug", Document::from(slug.clone())),
                (
                    "coords",
                    Document::from_typed(&locations.get(npc).cloned().unwrap_or_default())?,
                ),
                (
                    "price_copper",
                    Document::from_typed(&price.and_then(parse_price))?,
                ),
                ("price_raw", Document::from(price)),
                ("status_raw", Document::from(code)),
                ("raw", raw),
                ("status", Document::from_typed(&status)?),
                ("basis", Document::from_typed(&basis)?),
                ("sale_basis", Document::from("1.12 baseline")),
                ("price_basis", Document::from("1.12 baseline")),
            ]);
            rows.push(Payload::<Seller>::decode(d, "ninja seller")?);
        }
    }
    rows.sort_by_key(|r| {
        (
            crate::primitives::casefold(&r.view().zone),
            crate::primitives::casefold(&r.view().name),
            r.view().npc_id,
        )
    });
    Ok(rows)
}
#[derive(Deserialize)]
struct HttpFailure {
    status: Option<i64>,
}
fn status(e: &Failure) -> Result<Option<i64>> {
    e.body
        .as_ref()
        .map(|d| d.decode::<HttpFailure>("HTTP failure").map(|v| v.status))
        .transpose()
        .map(Option::flatten)
}
fn fetch(item_id: i64, context: &Context<'_>) -> Result<NinjaAnswer> {
    let get = |suffix: &str| -> Result<Option<(Document, Option<String>)>> {
        match context.http.request(
            &Request::get(format!(
                "{}/items/{item_id}.{suffix}",
                context.config.vendor_url
            ))
            .header("User-Agent", "Mozilla/5.0 (compatible; everledger)"),
        ) {
            Ok(r) => Ok(Some((
                crate::http::decode_body(&r.body)?,
                r.header("Last-Modified").map(str::to_owned),
            ))),
            Err(e) => {
                if status(&e)? == Some(404) {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    };
    let item = get("json")?;
    let pins = if item
        .as_ref()
        .is_some_and(|(d, _)| !matches!(d, Document::Null))
    {
        get("pins.json")?
    } else {
        None
    };
    let modified = pins
        .as_ref()
        .and_then(|(_, h)| h.as_deref())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            item.as_ref()
                .and_then(|(_, h)| h.as_deref())
                .filter(|s| !s.is_empty())
        });
    let published = modified
        .map(|s| -> Result<String> {
            let z = jiff::fmt::rfc2822::parse(s)
                .map_err(|e| Failure::shape("ninja Last-Modified", e))?;
            Ok(crate::time::iso(crate::time::UtcInstant::from_epoch(
                crate::EpochSeconds(z.timestamp().as_second()),
            )?))
        })
        .transpose()?;
    let item_doc = item.map(|(d, _)| d).unwrap_or(Document::Null);
    let pins_doc = pins.map(|(d, _)| d).unwrap_or(Document::Null);
    let item_view: Option<NinjaItem> = item_doc.decode("ninja item")?;
    let reason = if item_view.is_none() {
        Some("item not in dataset")
    } else if matches!(pins_doc, Document::Null) {
        Some("no pins published")
    } else {
        None
    };
    let sellers = if matches!(pins_doc, Document::Null) {
        Document::Null
    } else {
        Document::Array(
            normalize(pins_doc.clone())?
                .into_iter()
                .map(|r| r.into_document())
                .collect(),
        )
    };
    NinjaAnswer::decode(Document::object([
        (
            "item",
            Document::object([
                ("id", Document::from(item_id)),
                ("name", Document::from(item_view.and_then(|i| i.name))),
            ]),
        ),
        ("vendors", sellers),
        ("reason", Document::from(reason)),
        ("published_at", Document::from(published)),
        (
            "fetched_at",
            Document::from(crate::time::iso(context.clock.now())),
        ),
        ("raw_item", item_doc),
        ("raw_pins", pins_doc),
        ("backend", Document::from(BACKEND)),
        (
            "backend_url",
            Document::from(context.config.vendor_url.clone()),
        ),
    ]))
}
fn write_cache(path: &std::path::Path, data: &Document) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Failure::error("cache path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, crate::document::dumps(data, JsonMode::Stdout))?;
    std::fs::rename(temp, path)?;
    Ok(())
}
pub fn ninja_lookup(item_id: i64, refresh: bool, context: &Context<'_>) -> Result<NinjaAnswer> {
    let path = context
        .config
        .data_dir
        .join("vendors")
        .join(BACKEND)
        .join(format!("{item_id}.json"));
    let cached = match std::fs::read_to_string(&path) {
        Ok(s) => {
            let d = crate::document::parse(&s)?;
            if d.truthy() {
                #[derive(Deserialize)]
                struct Origin {
                    backend_url: Option<String>,
                }
                let origin: Origin = d.decode("ninja cache origin")?;
                if origin.backend_url.as_deref() == Some(&context.config.vendor_url) {
                    Some(NinjaAnswer::decode(d)?)
                } else {
                    None
                }
            } else {
                None
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let mut stale = false;
    let mut answer = if !refresh && cached.is_some() {
        let c = cached.ok_or_else(|| Failure::error("missing cache"))?;
        stale = crate::time::age(Some(&c.payload.view().fetched_at), context.clock)
            .ok_or_else(|| Failure::shape("ninja cache", "invalid fetched_at"))?
            > 86400;
        c
    } else {
        match fetch(item_id, context) {
            Ok(a) => {
                write_cache(&path, a.payload.document())?;
                a
            }
            Err(e) => match cached {
                Some(c)
                    if crate::time::age(Some(&c.payload.view().fetched_at), context.clock)
                        .is_some_and(|a| a <= 7 * 86400)
                        && status(&e)?.is_none_or(|s| s >= 500) =>
                {
                    stale = true;
                    c
                }
                Some(_) | None => return Err(e),
            },
        }
    };
    let mut seen = 0;
    let mut baseline = 0;
    let mut unverified = 0;
    let mut pins = 0;
    for r in answer.coverage.rows() {
        match r.basis {
            Basis::Seen => seen += 1,
            Basis::Baseline => baseline += 1,
            Basis::Unverified => unverified += 1,
        };
        pins += r.coords.len();
    }
    answer.payload.set("stale", stale)?;
    let mut source = crate::provenance::source(
        &format!("{BACKEND} pins"),
        answer.payload.view().published_at.as_deref(),
        context.clock,
    )?;
    source.set("fetched_at", answer.payload.view().fetched_at.clone())?;
    source.set("stale", stale)?;
    source.set("detail",format!("1.12 baseline sale/price; {seen} vendor seen in Forever, {baseline} baseline, {unverified} unverified; {} vendors, {pins} pins; faction unavailable",answer.coverage.rows().len()))?;
    answer
        .payload
        .set("sources", Document::Array(vec![source.into_document()]))?;
    Ok(answer)
}
pub fn sources(answer: &NinjaAnswer) -> Result<Vec<Source>> {
    crate::provenance::sources(answer.payload.document())
}
