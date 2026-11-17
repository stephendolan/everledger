#[path = "foreverdb_interrupt.rs"]
mod interrupt;
#[path = "foreverdb_mapping.rs"]
pub mod mapping;
#[path = "foreverdb_transport.rs"]
pub mod transport;
use crate::{
    Context, Failure, Result,
    catalogue::OrderedMap,
    clock::Clock,
    config::DailyRequestCap,
    document::{Document, JsonMode, dumps},
    http::Payload,
    provenance::{Source, source},
    time::{UtcInstant, age, instant, iso},
};
use mapping::{Basis, Identifier, Metadata};
use serde::Deserialize;
use std::{
    borrow::Cow,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
pub const BACKEND: &str = "foreverdb.net";
pub const FORMAT: i64 = 2;
const INTERVAL: f64 = 2.1;
#[derive(Clone, Debug, Deserialize)]
pub struct Item {
    pub id: i64,
    pub name: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Seller {
    pub npc_id: i64,
    pub name: String,
    pub zone: String,
    pub basis: Basis,
    pub title: Option<crate::primitives::Scalar>,
    pub zone_slug: String,
    pub side: Option<crate::primitives::Scalar>,
    pub locations: Vec<Location>,
    pub coords: Vec<Vec<crate::primitives::Scalar>>,
    pub price_copper: Option<crate::Copper>,
    pub stack_price_copper: Option<crate::Copper>,
    pub stack: i64,
    pub limited: bool,
    #[serde(rename = "limitedStockCount")]
    pub limited_stock_count: Option<i64>,
    pub sale_basis: String,
    pub vendor_basis: Basis,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Location {
    pub zone: String,
    pub zone_slug: String,
    pub x: Option<crate::primitives::Scalar>,
    pub y: Option<crate::primitives::Scalar>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct ItemEntry {
    pub item: Item,
    pub vendors: Vec<Seller>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct IndexData {
    pub format: i64,
    pub fetched_at: String,
    pub published_at: Option<String>,
    pub record_count: usize,
    pub vendor_total: usize,
    pub item_count: usize,
    pub items: OrderedMap<ItemEntry>,
    pub details: OrderedMap<Detail>,
    pub sync_id: Option<String>,
}
pub type Index = Payload<IndexData>;
#[derive(Clone, Debug, Deserialize)]
pub struct Attempt {
    pub attempted_at: String,
    #[serde(default)]
    pub requests: u64,
    pub sync_id: Option<String>,
    #[serde(default)]
    pub spends: Vec<Spend>,
    #[serde(default)]
    pub failed: bool,
    pub status: Option<u16>,
    pub finished_at: Option<String>,
    pub last_request_at: Option<String>,
    #[serde(default)]
    pub total_requests: u64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Spend {
    pub requests: u64,
    pub finished_at: String,
}
#[derive(Clone, Copy)]
struct RequestBudget(u64);
impl RequestBudget {
    fn remaining(cap: DailyRequestCap, spends: &[Spend]) -> Self {
        Self(spends.iter().fold(cap.get(), |remaining, spend| {
            remaining.saturating_sub(spend.requests)
        }))
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct Detail {
    pub data: Sales,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Sales {
    pub sells: Vec<MappedSale>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct MappedSale {
    pub item: Item,
    pub row: Seller,
}
#[derive(Clone, Debug, Deserialize)]
struct Retry {
    not_before: String,
}
#[derive(Clone, Debug, Deserialize)]
struct Pending {
    sync_id: String,
    next_offset: Option<usize>,
    vendor_ids: Vec<String>,
    details: OrderedMap<Detail>,
    metadata: Option<Metadata>,
    total_requests: u64,
    retry: Option<Retry>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct SyncData {
    pub configured: bool,
    pub complete: bool,
    #[serde(default)]
    pub requests: u64,
    #[serde(default)]
    pub failed: bool,
}
pub type SyncResult = Payload<SyncData>;
pub fn cache_dir(context: &Context<'_>) -> PathBuf {
    context.config.data_dir.join("vendors").join(BACKEND)
}
pub fn read(context: &Context<'_>, name: &str) -> Result<Option<Document>> {
    read_at(&cache_dir(context), name, context.clock)
}
fn read_at(dir: &Path, name: &str, clock: &dyn Clock) -> Result<Option<Document>> {
    let mut data = match std::fs::read_to_string(dir.join(name)) {
        Ok(s) => crate::document::parse(&s)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if name == "attempt.json" {
        let attempt: Attempt = data.decode("ForeverDB attempt")?;
        if data.get("spends").is_none() || attempt.finished_at.as_deref().is_none_or(str::is_empty)
        {
            let finished = attempt
                .finished_at
                .as_deref()
                .filter(|s| !s.is_empty())
                .or(attempt.last_request_at.as_deref().filter(|s| !s.is_empty()))
                .unwrap_or(&attempt.attempted_at);
            let mut spends = data
                .get("spends")
                .and_then(Document::as_array)
                .unwrap_or_default()
                .to_vec();
            spends.push(spend_document(attempt.requests, finished)?);
            data.set("spends", Document::Array(spends))?;
        }
        prune_spends(&mut data, clock)?;
    }
    Ok(Some(data))
}
fn spend_document(requests: u64, finished_at: &str) -> Result<Document> {
    Ok(Document::object([
        ("requests", Document::from_typed(&requests)?),
        ("finished_at", finished_at.into()),
    ]))
}
fn prune_spends(attempt: &mut Document, clock: &dyn Clock) -> Result<()> {
    let raw = attempt
        .get("spends")
        .and_then(Document::as_array)
        .ok_or_else(|| Failure::shape("ForeverDB spends", "expected array"))?;
    let mut retained = Vec::new();
    for doc in raw {
        let spend: Spend = doc.decode("ForeverDB spend")?;
        let seconds = age(Some(&spend.finished_at), clock)
            .ok_or_else(|| Failure::shape("ForeverDB spend", "invalid finished_at"))?;
        if seconds < 86400 {
            retained.push(doc.clone());
        }
    }
    attempt.set("spends", Document::Array(retained))
}
pub(super) fn finish_attempt(attempt: &mut Document, now: UtcInstant) -> Result<()> {
    let state: Attempt = attempt.decode("ForeverDB attempt")?;
    let finished = iso(now);
    let mut spends = attempt
        .get("spends")
        .and_then(Document::as_array)
        .ok_or_else(|| Failure::shape("ForeverDB spends", "expected array"))?
        .to_vec();
    if state.finished_at.as_deref().is_some_and(|s| !s.is_empty()) {
        spends.pop();
    }
    spends.push(spend_document(state.requests, &finished)?);
    attempt.set("finished_at", finished)?;
    attempt.set("spends", Document::Array(spends))
}
pub fn attempt(context: &Context<'_>) -> Result<Option<Attempt>> {
    read(context, "attempt.json")?
        .map(|d| d.decode("ForeverDB attempt"))
        .transpose()
}
fn random_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|n| format!("{n:02x}")).collect())
}
pub(super) fn write_at(dir: &Path, name: &str, doc: &Document, clock: &dyn Clock) -> Result<()> {
    let doc = if name == "attempt.json" {
        let mut attempt = doc.clone();
        prune_spends(&mut attempt, clock)?;
        Cow::Owned(attempt)
    } else {
        Cow::Borrowed(doc)
    };
    let path = dir.join(format!(
        "{name}.{}.tmp",
        random_id()?.chars().take(8).collect::<String>()
    ));
    let result = (|| {
        // Owner-only, like the cache files the Python implementation writes beside these.
        let mut stream = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        stream.write_all(dumps(&doc, JsonMode::Stdout).as_bytes())?;
        stream.sync_all()?;
        std::fs::rename(&path, dir.join(name))?;
        Ok(())
    })();
    let _ = std::fs::remove_file(path);
    result
}
fn remove(dir: &Path, name: &str) -> Result<()> {
    match std::fs::remove_file(dir.join(name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn index(context: &Context<'_>) -> Result<Option<Index>> {
    if transport::Transport::select(context.config)?.is_none() {
        return Ok(None);
    }
    let Some(doc) = read(context, "index.json")? else {
        return Ok(None);
    };
    if doc.get("format").and_then(Document::as_i64) != Some(FORMAT) {
        return Ok(None);
    }
    let index: Index = Payload::decode(doc, "ForeverDB index")?;
    let v = index.view();
    if v.record_count != v.details.0.len()
        || v.record_count != v.vendor_total
        || v.item_count != v.items.0.len()
    {
        return Ok(None);
    }
    Ok(Some(index))
}
pub fn usable_index(index: &Index, attempt: Option<&Attempt>, context: &Context<'_>) -> bool {
    !attempt.is_some_and(|a| {
        a.failed
            && (age(Some(&index.view().fetched_at), context.clock).is_some_and(|a| a > 7 * 86400)
                || a.status.is_some_and(|s| s < 500))
    })
}
pub fn active_index(context: &Context<'_>) -> Result<Option<Index>> {
    let idx = index(context)?;
    let a = if idx.is_some() {
        attempt(context)?
    } else {
        None
    };
    Ok(idx.filter(|i| usable_index(i, a.as_ref(), context)))
}
pub fn sync(context: &Context<'_>) -> Result<SyncResult> {
    let Some(transport) = transport::Transport::select(context.config)? else {
        return Payload::decode(
            Document::object([
                ("configured", false.into()),
                ("complete", false.into()),
                ("requests", 0i64.into()),
            ]),
            "ForeverDB sync",
        );
    };
    let dir = cache_dir(context);
    std::fs::create_dir_all(&dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("sync.lock"))?;
    lock.lock()?;
    sync_locked(context, &transport, &dir)
}
fn sync_locked(
    context: &Context<'_>,
    transport: &transport::Transport,
    dir: &Path,
) -> Result<SyncResult> {
    let previous = index(context)?;
    let old_record = read(context, "attempt.json")?;
    let old_attempt: Option<Attempt> = old_record
        .as_ref()
        .map(|d| d.decode("ForeverDB attempt"))
        .transpose()?;
    let mut pending = read(context, "pending.json")?;
    let budget = RequestBudget::remaining(
        context.config.foreverdb_daily_requests,
        old_attempt.as_ref().map_or(&[], |attempt| &attempt.spends),
    );
    if budget.0 == 0 {
        return Payload::decode(
            Document::object([
                ("configured", true.into()),
                ("complete", (previous.is_some() && pending.is_none()).into()),
                ("pending", pending.is_some().into()),
                ("requests", 0i64.into()),
                ("deferred", "daily sync limit".into()),
                (
                    "failed",
                    old_attempt.as_ref().is_some_and(|a| a.failed).into(),
                ),
            ]),
            "ForeverDB sync",
        );
    }
    if pending
        .as_ref()
        .is_some_and(|d| d.get("format").and_then(Document::as_i64) != Some(FORMAT))
    {
        pending = None;
    }
    if let (Some(p), Some(old)) = (&pending, &previous)
        && p.get("sync_id").and_then(Document::as_str) == old.view().sync_id.as_deref()
    {
        remove(dir, "pending.json")?;
        pending = None;
    }
    let pending = match pending {
        Some(p) => p,
        None => Document::object([
            ("format", FORMAT.into()),
            ("sync_id", random_id()?.into()),
            ("started_at", iso(context.clock.now()).into()),
            ("next_offset", 0i64.into()),
            ("vendor_ids", Document::Array(vec![])),
            ("details", Document::Object(vec![])),
            ("metadata", Document::Null),
            ("total_requests", 0i64.into()),
        ]),
    };
    let mut pending: Payload<Pending> = Payload::decode(pending, "ForeverDB checkpoint")?;
    if let Some(a) = &old_attempt
        && a.sync_id.as_deref() == Some(&pending.view().sync_id)
    {
        let total = pending.view().total_requests.max(a.total_requests);
        pending.set("total_requests", Document::from_typed(&total)?)?;
    }
    let mut attempt: Payload<Attempt> = Payload::decode(
        Document::object([
            ("attempted_at", iso(context.clock.now()).into()),
            ("requests", 0i64.into()),
            ("sync_id", pending.view().sync_id.clone().into()),
            (
                "spends",
                old_record
                    .as_ref()
                    .and_then(|d| d.get("spends").cloned())
                    .unwrap_or(Document::Array(vec![])),
            ),
            (
                "failed",
                old_attempt.as_ref().is_some_and(|a| a.failed).into(),
            ),
            (
                "status",
                old_attempt
                    .as_ref()
                    .and_then(|a| a.status)
                    .map(i64::from)
                    .into(),
            ),
        ]),
        "ForeverDB attempt",
    )?;
    write_at(dir, "attempt.json", attempt.document(), context.clock)?;
    let interrupt = interrupt::Guard::new(dir)?;
    let mut engine = Engine {
        context,
        transport,
        pending: &mut pending,
        attempt: &mut attempt,
        budget,
        last_start: None,
        interrupt: &interrupt,
    };
    engine.checkpoint_interrupt()?;
    let mut obsolete = false;
    let result = (|| {
        transport.validate(context)?;
        while let Some(offset) = engine.pending.view().next_offset {
            if engine.attempt.view().requests >= engine.budget.0 {
                break;
            }
            let Some(doc) = engine.get(&format!(
                "/api/v1/vendors?limit={}&offset={offset}",
                mapping::PAGE_SIZE
            ))?
            else {
                break;
            };
            let page = mapping::page(doc, offset)?;
            if engine
                .pending
                .view()
                .metadata
                .as_ref()
                .is_some_and(|m| *m != page.metadata)
            {
                obsolete = true;
                return Err(mapping::invalid());
            }
            if page
                .ids
                .iter()
                .any(|id| engine.pending.view().vendor_ids.contains(id))
            {
                return Err(mapping::invalid());
            }
            engine.pending.set("metadata", page.metadata.document())?;
            let mut ids = engine.pending.view().vendor_ids.clone();
            ids.extend(page.ids);
            engine.pending.set(
                "vendor_ids",
                Document::Array(ids.into_iter().map(Document::from).collect()),
            )?;
            engine.pending.set("next_offset", page.next)?;
            interrupt.write("pending.json", engine.pending.document(), context.clock)?;
        }
        if engine.pending.view().next_offset.is_none() {
            for id in engine.pending.view().vendor_ids.clone() {
                if engine.pending.view().details.get(&id).is_some() {
                    continue;
                }
                if engine.attempt.view().requests >= engine.budget.0 {
                    break;
                }
                let Some(doc) = engine.get(&format!("/api/v1/vendors/{id}"))? else {
                    break;
                };
                let expected =
                    Identifier::try_from(id.parse::<i64>().map_err(|_| mapping::invalid())?)
                        .map_err(|_| mapping::invalid())?;
                let data = mapping::vendor(doc, expected)?;
                engine
                    .pending
                    .set_path(&["details", &id], Document::object([("data", data)]))?;
                interrupt.write("pending.json", engine.pending.document(), context.clock)?;
            }
        }
        let complete = engine.pending.view().next_offset.is_none()
            && engine.pending.view().details.0.len() == engine.pending.view().vendor_ids.len();
        if complete {
            let mut data = snapshot(engine.pending, previous.as_ref(), context)?;
            data.set("sync_id", engine.pending.view().sync_id.clone())?;
            interrupt.write("index.json", &data, context.clock)?;
            interrupt.remove("pending.json")?;
            engine.attempt.set("failed", false)?;
            engine.attempt.set("status", Document::Null)?;
        } else {
            interrupt.write("pending.json", engine.pending.document(), context.clock)?;
        }
        engine.finish()?;
        interrupt.write("attempt.json", engine.attempt.document(), context.clock)?;
        Payload::decode(
            Document::object([
                ("configured", true.into()),
                ("complete", complete.into()),
                ("pending", (!complete).into()),
                (
                    "requests",
                    Document::from_typed(&engine.attempt.view().requests)?,
                ),
                (
                    "total_requests",
                    Document::from_typed(&engine.pending.view().total_requests)?,
                ),
            ]),
            "ForeverDB sync",
        )
    })();
    if let Err(e) = &result {
        engine.finish()?;
        engine.attempt.set("failed", true)?;
        engine.attempt.set(
            "status",
            e.body
                .as_ref()
                .and_then(|d| d.get("status"))
                .cloned()
                .unwrap_or(Document::Null),
        )?;
        interrupt.write("attempt.json", engine.attempt.document(), context.clock)?;
        if obsolete {
            interrupt.remove("pending.json")?;
        } else {
            interrupt.write("pending.json", engine.pending.document(), context.clock)?;
        }
    }
    interrupt.clear();
    result
}
struct Engine<'a, 'b> {
    context: &'a Context<'b>,
    transport: &'a transport::Transport,
    pending: &'a mut Payload<Pending>,
    attempt: &'a mut Payload<Attempt>,
    budget: RequestBudget,
    last_start: Option<f64>,
    interrupt: &'a interrupt::Guard,
}
impl Engine<'_, '_> {
    fn finish(&mut self) -> Result<()> {
        let mut doc = self.attempt.document().clone();
        finish_attempt(&mut doc, self.context.clock.now())?;
        *self.attempt = Payload::decode(doc, "ForeverDB attempt")?;
        Ok(())
    }
    fn checkpoint_interrupt(&self) -> Result<()> {
        self.interrupt.update(
            self.attempt.document(),
            self.pending.document(),
            self.context.clock.now(),
        );
        Ok(())
    }
    fn get(&mut self, path: &str) -> Result<Option<Document>> {
        loop {
            if self.attempt.view().requests >= self.budget.0 {
                return Ok(None);
            }
            let retry = self.pending.view().retry.clone();
            self.checkpoint_interrupt()?;
            if let Some(retry) = &retry {
                let delay = (instant(&retry.not_before)?.microseconds()
                    - self.context.clock.now().microseconds()) as f64
                    / 1e6;
                self.context.clock.sleep(delay.max(0.0))?;
            }
            if let Some(last) = self.last_start {
                self.context
                    .clock
                    .sleep((INTERVAL - (self.context.clock.monotonic() - last)).max(0.0))?;
            }
            self.attempt.set(
                "requests",
                Document::from_typed(&(self.attempt.view().requests + 1))?,
            )?;
            let total = self
                .pending
                .view()
                .total_requests
                .checked_add(1)
                .ok_or_else(mapping::invalid)?;
            self.pending
                .set("total_requests", Document::from_typed(&total)?)?;
            self.attempt
                .set("total_requests", Document::from_typed(&total)?)?;
            self.attempt
                .set("last_request_at", iso(self.context.clock.now()))?;
            self.interrupt
                .write("attempt.json", self.attempt.document(), self.context.clock)?;
            self.last_start = Some(self.context.clock.monotonic());
            self.checkpoint_interrupt()?;
            match self.transport.request(path, self.context) {
                Ok(raw) => {
                    self.pending.remove("retry")?;
                    return crate::http::decode_body(&raw)
                        .map(Some)
                        .map_err(|_| mapping::invalid());
                }
                Err(error) => {
                    let body = error.body.as_ref();
                    let status = body
                        .and_then(|d| d.get("status"))
                        .and_then(Document::as_i64);
                    let response = body.and_then(|d| d.get("response"));
                    if status == Some(502)
                        && retry.is_none()
                        && response.and_then(|d| d.get("retryable")) == Some(&Document::Bool(true))
                    {
                        let delay = response
                            .and_then(|d| d.get("retry_after"))
                            .filter(|d| matches!(d, Document::Number(_)))
                            .and_then(Document::as_f64)
                            .filter(|n| n.is_finite() && *n >= 0.0)
                            .unwrap_or(60.0)
                            .max(60.0)
                            .ceil()
                            + 1.0;
                        if delay > (i64::MAX / 1_000_000) as f64 {
                            return Err(Failure::unreachable("ForeverDB sync failed (HTTP 502)"));
                        }
                        let not_before = iso(self
                            .context
                            .clock
                            .now()
                            .add_microseconds((delay * 1e6) as i64)?);
                        self.pending.set(
                            "retry",
                            Document::object([("not_before", not_before.into())]),
                        )?;
                        self.interrupt.write(
                            "pending.json",
                            self.pending.document(),
                            self.context.clock,
                        )?;
                        continue;
                    }
                    self.pending.remove("retry")?;
                    return Err(Failure {
                        message: crate::human::refresh::sync_failed(status),
                        code: crate::ExitCode::Unreachable,
                        body: Some(Document::object([("status", status.into())])),
                    });
                }
            }
        }
    }
}
type SnapshotItem = (String, Document, Vec<(Seller, Document)>);
fn snapshot(
    pending: &Payload<Pending>,
    previous: Option<&Index>,
    context: &Context<'_>,
) -> Result<Document> {
    let mut entries: Vec<SnapshotItem> = vec![];
    if let Some(index) = previous {
        for (id, _) in &index.view().items.0 {
            let item = index
                .document()
                .get("items")
                .and_then(|d| d.get(id))
                .and_then(|d| d.get("item"))
                .cloned()
                .ok_or_else(mapping::invalid)?;
            entries.push((id.clone(), item, vec![]));
        }
    }
    let detail_docs = pending
        .document()
        .get("details")
        .ok_or_else(mapping::invalid)?;
    for (id, d) in &pending.view().details.0 {
        let docs = detail_docs
            .get(id)
            .and_then(|d| d.get("data"))
            .and_then(|d| d.get("sells"))
            .and_then(Document::as_array)
            .ok_or_else(mapping::invalid)?;
        for (sale, doc) in d.data.sells.iter().zip(docs) {
            let key = sale.item.id.to_string();
            let pos = if let Some(pos) = entries.iter().position(|(k, _, _)| *k == key) {
                pos
            } else {
                entries.push((
                    key,
                    doc.get("item").cloned().ok_or_else(mapping::invalid)?,
                    vec![],
                ));
                entries.len() - 1
            };
            entries[pos].2.push((
                sale.row.clone(),
                doc.get("row").cloned().ok_or_else(mapping::invalid)?,
            ));
        }
    }
    for (_, _, rows) in &mut entries {
        rows.sort_by_cached_key(|(r, _)| {
            (
                crate::primitives::casefold(&r.zone),
                crate::primitives::casefold(&r.name),
                r.npc_id,
            )
        });
    }
    let count = entries.len();
    let items = Document::object(entries.into_iter().map(|(key, item, rows)| {
        (
            key,
            Document::object([
                ("item", item),
                (
                    "vendors",
                    Document::Array(rows.into_iter().map(|(_, d)| d).collect()),
                ),
            ]),
        )
    }));
    let meta = pending
        .view()
        .metadata
        .as_ref()
        .ok_or_else(mapping::invalid)?;
    Ok(Document::object([
        ("format", FORMAT.into()),
        ("backend", BACKEND.into()),
        ("fetched_at", iso(context.clock.now()).into()),
        ("build", meta.build.clone().into()),
        ("vendor_total", meta.vendor_total.into()),
        ("published_at", meta.published_at.as_deref().into()),
        ("record_count", pending.view().details.0.len().into()),
        ("item_count", count.into()),
        ("items", items),
        ("details", detail_docs.clone()),
        (
            "sync_requests",
            Document::from_typed(&pending.view().total_requests)?,
        ),
    ]))
}
pub fn provenance(
    index: &Index,
    rows: &[Seller],
    stale: bool,
    context: &Context<'_>,
) -> Result<Vec<Source>> {
    let counts = [Basis::Seen, Basis::Baseline, Basis::Unverified]
        .map(|basis| rows.iter().filter(|r| r.basis == basis).count());
    let mut s = source(
        "foreverdb.net vendors",
        index.view().published_at.as_deref(),
        context.clock,
    )?;
    s.set("fetched_at", index.view().fetched_at.clone())?;
    s.set("stale", stale)?;
    s.set("detail", crate::human::refresh::credit(counts))?;
    Ok(vec![s])
}
pub fn index_sources(index: &Index, context: &Context<'_>) -> Result<Vec<Source>> {
    let rows = index
        .view()
        .items
        .0
        .iter()
        .flat_map(|(_, e)| e.vendors.clone())
        .collect::<Vec<_>>();
    provenance(index, &rows, stale(index, context)?, context)
}
fn stale(index: &Index, context: &Context<'_>) -> Result<bool> {
    Ok(
        age(Some(&index.view().fetched_at), context.clock).is_some_and(|s| s > 86400)
            || attempt(context)?.is_some_and(|a| a.failed),
    )
}
pub fn lookup(item_id: i64, index: &Index, context: &Context<'_>) -> Result<Document> {
    let key = item_id.to_string();
    let entry = index.view().items.get(&key);
    let docs = index.document().get("items").and_then(|d| d.get(&key));
    let stale = stale(index, context)?;
    let sources = provenance(
        index,
        entry.map_or(&[], |e| e.vendors.as_slice()),
        stale,
        context,
    )?;
    Ok(Document::object([
        (
            "item",
            docs.and_then(|d| d.get("item"))
                .cloned()
                .unwrap_or_else(|| {
                    Document::object([("id", item_id.into()), ("name", Document::Null)])
                }),
        ),
        (
            "vendors",
            docs.and_then(|d| d.get("vendors"))
                .cloned()
                .unwrap_or(Document::Null),
        ),
        (
            "reason",
            if entry.is_some() {
                Document::Null
            } else {
                "item not in dataset".into()
            },
        ),
        ("backend", BACKEND.into()),
        ("published_at", index.view().published_at.as_deref().into()),
        ("fetched_at", index.view().fetched_at.clone().into()),
        ("stale", stale.into()),
        (
            "sources",
            Document::Array(sources.into_iter().map(Payload::into_document).collect()),
        ),
    ]))
}
