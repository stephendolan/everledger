use crate::foreverdb::{
    AttemptSummary, CacheError, CacheResult, Index, Item, transport::Transport,
};
use crate::{
    Context, Copper, Failure, Result,
    catalogue::{OrderedMap, nullable},
    document::{Document, JsonMode},
    http::{Payload, Request},
    primitives::Scalar,
    provenance::{Field, Source},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
mod foreverdb_answer;
pub use foreverdb_answer::ForeverdbAnswer;
pub mod status;
pub const BACKEND: &str = "wowforever.ninja";
const USER_AGENT: &str = "Mozilla/5.0 (compatible; everledger)";
pub fn ninja_request(url: impl Into<String>) -> Request {
    Request::public_get(url).header("User-Agent", USER_AGENT)
}
#[derive(Clone, Copy)]
pub(crate) struct CacheAge(i64);
impl CacheAge {
    pub(crate) fn fetched_at(value: &str, boundary: &str, context: &Context<'_>) -> Result<Self> {
        crate::time::age(Some(value), context.clock)
            .map(Self)
            .ok_or_else(|| Failure::shape(boundary, "invalid fetched_at"))
    }
    pub(crate) fn stale(self) -> bool {
        self.0 > 86400
    }
    pub(crate) fn within_failure_window(self) -> bool {
        self.0 <= 7 * 86400
    }
}
pub(crate) fn retryable_status(status: Option<i64>) -> bool {
    status.is_none_or(|s| s >= 500)
}
enum Journal {
    Unread,
    Loaded(Option<AttemptSummary>),
}
pub(crate) struct BackendState {
    transport: Option<Transport>,
    pub(crate) completed: Option<Index>,
    journal: Journal,
}
impl BackendState {
    pub(crate) fn load(context: &Context<'_>) -> CacheResult<Self> {
        let transport = Transport::select(context.config).map_err(CacheError::Failure)?;
        let completed = if transport.is_some() {
            crate::foreverdb::cached_index(context)?
        } else {
            None
        };
        Ok(Self {
            transport,
            completed,
            journal: Journal::Unread,
        })
    }
    fn attempt(&self) -> Option<&AttemptSummary> {
        match &self.journal {
            Journal::Unread => None,
            Journal::Loaded(attempt) => attempt.as_ref(),
        }
    }
    fn failed(&mut self, context: &Context<'_>) -> CacheResult<bool> {
        if let Journal::Unread = self.journal {
            self.journal = Journal::Loaded(if self.transport.is_some() {
                crate::foreverdb::cached_attempt(context)?
            } else {
                None
            });
        }
        Ok(self.attempt().is_some_and(|a| a.failed))
    }
    fn state(&mut self, context: &Context<'_>) -> CacheResult<status::ForeverdbState> {
        use status::ForeverdbState;
        let failed = self.failed(context)?;
        let usable = match (&self.completed, self.attempt().filter(|a| a.failed)) {
            (Some(index), Some(attempt)) => {
                crate::foreverdb::index_age(index, context)?.within_failure_window()
                    && retryable_status(attempt.status.map(i64::from))
            }
            (Some(_), None) => true,
            (None, _) => false,
        };
        Ok(match (self.transport.is_some(), failed, usable) {
            (_, true, true) => ForeverdbState::ServingPrevious,
            (_, true, false) => ForeverdbState::NinjaFallback,
            (false, false, _) => ForeverdbState::NotConfigured,
            (true, false, true) => ForeverdbState::Synced,
            (true, false, false) => ForeverdbState::Awaiting,
        })
    }
    fn serving_index(&self, state: status::ForeverdbState) -> Option<&Index> {
        use status::ForeverdbState;
        match state {
            ForeverdbState::Synced | ForeverdbState::ServingPrevious => self.completed.as_ref(),
            ForeverdbState::NotConfigured
            | ForeverdbState::Awaiting
            | ForeverdbState::NinjaFallback => None,
        }
    }
    fn active_index(
        &mut self,
        context: &Context<'_>,
    ) -> CacheResult<Option<(&Index, Option<&AttemptSummary>)>> {
        if self.completed.is_none() {
            return Ok(None);
        }
        let state = self.state(context)?;
        Ok(self
            .serving_index(state)
            .map(|index| (index, self.attempt())))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Unchanged,
    Changed,
    New,
    Baseline,
    Unverified,
}
pub use crate::foreverdb::mapping::Basis;
pub(crate) fn index_bases(index: &Index) -> impl Iterator<Item = Basis> {
    index
        .view()
        .items
        .0
        .iter()
        .flat_map(|(_, entry)| &entry.vendors)
        .map(|row| row.basis)
}
pub(crate) fn basis_counts(bases: impl IntoIterator<Item = Basis>) -> [usize; 3] {
    let mut counts = [0; 3];
    for basis in bases {
        counts[match basis {
            Basis::Seen => 0,
            Basis::Baseline => 1,
            Basis::Unverified => 2,
        }] += 1;
    }
    counts
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
    Sellers,
}
#[derive(Clone, Debug)]
pub struct NinjaAnswer {
    payload: Payload<Snapshot>,
    coverage: Coverage,
}
impl NinjaAnswer {
    fn age(&self, context: &Context<'_>) -> Result<CacheAge> {
        CacheAge::fetched_at(&self.payload.view().fetched_at, "ninja cache", context)
    }
    pub fn payload(&self) -> &Payload<Snapshot> {
        &self.payload
    }
    pub fn rows(&self) -> &[Seller] {
        self.payload.view().vendors.as_deref().unwrap_or(&[])
    }
    pub fn coverage(&self) -> &Coverage {
        &self.coverage
    }
    fn decode(doc: Document) -> Result<Self> {
        let payload: Payload<Snapshot> = Payload::decode(doc, "ninja snapshot")?;
        let coverage = match (&payload.view().vendors, payload.view().reason.as_deref()) {
            (None, Some("item not in dataset")) => Coverage::ItemAbsent,
            (None, Some("no pins published")) => Coverage::PinsAbsent,
            (Some(_), None) => Coverage::Sellers,
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
            .rows()
            .iter()
            .map(|r| r.zone.clone())
            .collect::<BTreeSet<_>>();
        let query = crate::primitives::casefold(query);
        let q = zone_query(&query);
        if let Coverage::Sellers = self.coverage {
            let docs = self
                .payload
                .document()
                .get("vendors")
                .and_then(Document::as_array)
                .ok_or_else(|| Failure::shape("ninja snapshot", "missing seller documents"))?;
            let filtered = self
                .rows()
                .iter()
                .zip(docs)
                .filter(|(r, _)| matches_zone(&r.zone, &r.zone_slug, q))
                .map(|(_, d)| d.clone())
                .collect();
            self.payload.set("vendors", Document::Array(filtered))?;
        }
        self.payload
            .set("known_zones", Document::from_typed(&zones)?)?;
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub enum VendorAnswer {
    Ninja(NinjaAnswer),
    Foreverdb(ForeverdbAnswer),
}
impl VendorAnswer {
    pub fn select_zone(&mut self, query: &str) -> Result<()> {
        match self {
            Self::Ninja(a) => a.select_zone(query),
            Self::Foreverdb(a) => a.select_zone(query),
        }
    }
    fn document(&self) -> &Document {
        match self {
            Self::Ninja(a) => a.payload.document(),
            Self::Foreverdb(a) => a.payload().document(),
        }
    }
    pub fn public(&self) -> Document {
        let mut data = self.document().clone();
        let private: &[&str] = match self {
            Self::Ninja(_) => &["raw_item", "raw_pins", "backend_url", "_fallback_vendors"],
            Self::Foreverdb(_) => &["_fallback_vendors"],
        };
        for key in private {
            data.remove(key);
        }
        data
    }
    pub fn sellers_document(&self) -> Result<Document> {
        self.document().get("vendors").cloned().ok_or_else(|| {
            Failure::shape(
                match self {
                    Self::Ninja(_) => "ninja snapshot",
                    Self::Foreverdb(_) => "ForeverDB answer",
                },
                "missing vendors",
            )
        })
    }
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Ninja(a) => a.payload.view().reason.as_deref(),
            Self::Foreverdb(a) => a.payload().view().reason.as_deref(),
        }
    }
    pub fn sources(&self) -> Result<Vec<Source>> {
        crate::provenance::sources(self.document())
    }
}
pub fn lookup(item_id: i64, context: &Context<'_>) -> Result<VendorAnswer> {
    let mut backend = BackendState::load(context).map_err(CacheError::into_failure)?;
    if let Some((index, attempt)) = backend
        .active_index(context)
        .map_err(CacheError::into_failure)?
    {
        return Ok(VendorAnswer::Foreverdb(ForeverdbAnswer::lookup(
            item_id, index, attempt, context,
        )?));
    }
    let mut answer = ninja_lookup(item_id, false, context)?;
    if backend.failed(context).map_err(CacheError::into_failure)? {
        ninja_fallback(&mut answer, context).map_err(CacheError::into_failure)?;
    }
    Ok(VendorAnswer::Ninja(answer))
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
            let Some(digit) = c.to_digit(10).filter(|_| c.is_ascii()) else {
                break;
            };
            number = number.checked_mul(10)?.checked_add(i64::from(digit))?;
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
fn fetch(item_id: i64, context: &Context<'_>) -> CacheResult<NinjaAnswer> {
    let get = |suffix: &str| -> CacheResult<Option<(Document, Option<String>)>> {
        match context.http.request(&ninja_request(format!(
            "{}/items/{item_id}.{suffix}",
            context.config.vendor_url
        ))) {
            Ok(r) => Ok(Some((
                crate::http::decode_body(&r.body)?,
                r.header("Last-Modified").map(str::to_owned),
            ))),
            Err(e) => {
                if status(&e)? == Some(404) {
                    Ok(None)
                } else {
                    Err(CacheError::Failure(e))
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
    Ok(NinjaAnswer::decode(Document::object([
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
    ]))?)
}
fn write_cache(path: &std::path::Path, data: &Document) -> CacheResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Failure::error("cache path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        // Owner-only, like the cache files the Python implementation writes beside these.
        let mut stream = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        stream.write_all(crate::document::dumps(data, JsonMode::Stdout).as_bytes())?;
    }
    std::fs::rename(temp, path)?;
    Ok(())
}
pub fn ninja_lookup(item_id: i64, refresh: bool, context: &Context<'_>) -> Result<NinjaAnswer> {
    cached_ninja_lookup(item_id, refresh, context).map_err(CacheError::into_failure)
}
fn cached_ninja_lookup(
    item_id: i64,
    refresh: bool,
    context: &Context<'_>,
) -> CacheResult<NinjaAnswer> {
    let path = ninja_cache_path(item_id, context);
    let cached = read_ninja_cache(&path, NinjaRead::Lookup, context)?
        .map(NinjaAnswer::decode)
        .transpose()?;
    let (mut answer, stale) = match (refresh, cached) {
        (false, Some(c)) => {
            let stale = c.age(context)?.stale();
            (c, stale)
        }
        (_, cached) => match fetch(item_id, context) {
            Ok(a) => {
                write_cache(&path, a.payload.document())?;
                (a, false)
            }
            Err(CacheError::Failure(error)) => match cached {
                Some(c) => {
                    if !c.age(context)?.within_failure_window()
                        || !retryable_status(status(&error)?)
                    {
                        return Err(CacheError::Failure(error));
                    }
                    (c, true)
                }
                None => return Err(CacheError::Failure(error)),
            },
            Err(error) => return Err(error),
        },
    };
    let rows = answer.rows();
    let [seen, baseline, unverified] = basis_counts(rows.iter().map(|r| r.basis));
    let pins: usize = rows.iter().map(|r| r.coords.len()).sum();
    answer.payload.set("stale", stale)?;
    let mut source = crate::provenance::source(
        &format!("{BACKEND} pins"),
        answer.payload.view().published_at.as_deref(),
        context.clock,
    )?;
    source.set("fetched_at", answer.payload.view().fetched_at.clone())?;
    source.set("stale", stale)?;
    source.set("detail",format!("1.12 baseline sale/price; {seen} vendor seen in Forever, {baseline} baseline, {unverified} unverified; {} vendors, {pins} pins; faction unavailable",answer.rows().len()))?;
    answer
        .payload
        .set("sources", Document::Array(vec![source.into_document()]))?;
    Ok(answer)
}
#[derive(Clone, Copy)]
enum NinjaRead {
    Lookup,
    Comparison,
    Status,
}
fn ninja_cache_dir(context: &Context<'_>) -> std::path::PathBuf {
    context.config.data_dir.join("vendors").join(BACKEND)
}
fn ninja_cache_path(item_id: i64, context: &Context<'_>) -> std::path::PathBuf {
    ninja_cache_dir(context).join(format!("{item_id}.json"))
}
fn read_ninja_cache(
    path: &std::path::Path,
    mode: NinjaRead,
    context: &Context<'_>,
) -> CacheResult<Option<Document>> {
    let text = match mode {
        NinjaRead::Lookup => std::fs::read(path).map(|bytes| {
            String::from_utf8(bytes).map_err(|e| Failure::shape("ninja cache text", e))
        }),
        NinjaRead::Comparison | NinjaRead::Status => std::fs::read_to_string(path).map(Ok),
    };
    let text = match text {
        Ok(text) => text?,
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound && !matches!(mode, NinjaRead::Status) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    let doc = crate::document::parse(&text)?;
    if matches!(mode, NinjaRead::Lookup) && !doc.truthy() {
        return Ok(None);
    }
    crate::foreverdb::cache_object(&doc, "ninja cache origin")?;
    let origin: CacheOrigin = doc.decode("ninja cache origin")?;
    Ok((origin.backend_url.as_deref() == Some(&context.config.vendor_url)).then_some(doc))
}

#[derive(Deserialize)]
struct CacheOrigin {
    backend_url: Option<String>,
}
#[derive(Deserialize)]
struct CachedCoverage {
    vendors: Option<Vec<Seller>>,
    published_at: Option<String>,
}
fn cache_paths(context: &Context<'_>) -> Result<Vec<std::path::PathBuf>> {
    let dir = ninja_cache_dir(context);
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut paths = vec![];
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}
fn ninja_fallback(answer: &mut NinjaAnswer, context: &Context<'_>) -> CacheResult<()> {
    if !answer.age(context)?.within_failure_window() {
        return Err(CacheError::Failure(Failure::unreachable(
            "ForeverDB sync failed; ninja fallback cache is older than 7d",
        )));
    }
    answer.payload.set("stale", true)?;
    let mut sources = crate::provenance::sources(answer.payload.document())?;
    for source in &mut sources {
        source.set("stale", true)?;
        let detail = source
            .view()
            .detail
            .as_deref()
            .ok_or_else(|| Failure::shape("ninja provenance", "missing detail"))?;
        source.set("detail", crate::human::vendors::fallback_detail(detail))?;
    }
    answer.payload.set(
        "sources",
        Document::Array(sources.into_iter().map(Payload::into_document).collect()),
    )?;
    Ok(())
}
pub fn refresh_cached(
    context: &Context<'_>,
) -> Result<(Vec<Source>, crate::foreverdb::SyncResult)> {
    let synced = crate::foreverdb::sync(context)?;
    let index_sources = (|| {
        let mut backend = BackendState::load(context)?;
        backend
            .active_index(context)?
            .map(|(index, attempt)| {
                crate::foreverdb::index_sources(index, attempt, context)
                    .map_err(CacheError::Invalid)
            })
            .transpose()
    })();
    match index_sources {
        Ok(Some(sources)) => return Ok((sources, synced)),
        Ok(None) => {}
        Err(CacheError::Failure(_) | CacheError::Io(_)) if synced.view().failed => {}
        Err(error) => return Err(error.into_failure()),
    }
    let paths = cache_paths(context)?;
    let mut used = vec![];
    for path in paths {
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| Failure::shape("ninja cache path", "invalid item id"))?;
        let refreshed = (|| {
            let mut answer = cached_ninja_lookup(id, true, context)?;
            if synced.view().failed {
                ninja_fallback(&mut answer, context)?;
            }
            crate::provenance::sources(answer.payload.document()).map_err(CacheError::Invalid)
        })();
        match refreshed {
            Ok(sources) => used.extend(sources),
            Err(CacheError::Failure(_)) if synced.view().failed => {}
            Err(error) => return Err(error.into_failure()),
        }
    }
    Ok((used, synced))
}

fn zone_query(query: &str) -> &str {
    match query {
        "if" => "ironforge",
        "sw" => "stormwind",
        "darn" => "darnassus",
        "org" => "orgrimmar",
        "tb" => "thunder bluff",
        "uc" => "undercity",
        _ => query,
    }
}

fn matches_zone(zone: &str, slug: &str, query: &str) -> bool {
    crate::primitives::casefold(zone).contains(query)
        || crate::primitives::casefold(slug).contains(query)
}
