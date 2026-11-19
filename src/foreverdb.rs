#[path = "foreverdb_interrupt.rs"]
mod interrupt;
#[path = "foreverdb_mapping.rs"]
pub mod mapping;
#[path = "foreverdb_transport.rs"]
pub mod transport;
use crate::{
    Context, Failure, Result,
    catalogue::{OrderedMap, nullable},
    clock::Clock,
    config::DailyRequestCap,
    document::{Document, JsonMode, Number, dumps},
    http::Payload,
    provenance::{Source, source},
    time::{UtcInstant, age, instant, iso},
};
use mapping::{Basis, Identifier, Metadata};
use serde::{Deserialize, Serialize};
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
fn retry_at(value: Option<&Document>, now: UtcInstant) -> Result<UtcInstant> {
    let delay = match value {
        Some(Document::Number(Number::Small(n))) => *n as f64,
        Some(Document::Number(Number::Integer(text))) => {
            if text.starts_with('-') {
                60.0
            } else {
                text.parse::<f64>().unwrap_or(f64::INFINITY)
            }
        }
        Some(Document::Number(Number::Float(n))) if n.is_finite() && *n >= 0.0 => *n,
        Some(
            Document::Number(Number::Float(_))
            | Document::Null
            | Document::Bool(_)
            | Document::String(_)
            | Document::Array(_)
            | Document::Object(_),
        )
        | None => 60.0,
    };
    let last = instant("9999-12-31T23:59:59.999999Z")?;
    // Python divides the microseconds left by a million in one correctly rounded
    // step. Whole seconds plus the remainder's quotient is that same double;
    // converting the count first rounds twice and can land a second short.
    let left = last.microseconds() - now.microseconds();
    let maximum = (left / 1_000_000) as f64 + (left % 1_000_000) as f64 / 1e6 - 1.0;
    let seconds = delay.max(60.0).min(maximum).ceil() as i64;
    now.add_microseconds(seconds * 1_000_000)
}
fn retry_iso(value: UtcInstant) -> String {
    let micros = value.microseconds().rem_euclid(1_000_000);
    if micros == 0 {
        iso(value)
    } else {
        format!("{}.{micros:06}Z", iso(value).trim_end_matches('Z'))
    }
}
fn sync_io(_: std::io::Error) -> Failure {
    Failure::error("ForeverDB sync failed")
}

#[derive(Clone, Debug, Deserialize)]
pub struct Item {
    pub id: i64,
    #[serde(deserialize_with = "nullable")]
    pub name: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Seller {
    pub npc_id: i64,
    pub name: String,
    pub zone: String,
    pub basis: Basis,
    #[serde(deserialize_with = "nullable")]
    pub title: Option<crate::primitives::Scalar>,
    pub zone_slug: String,
    #[serde(deserialize_with = "nullable")]
    pub side: Option<crate::primitives::Scalar>,
    pub coords: Vec<Vec<crate::primitives::Scalar>>,
    #[serde(deserialize_with = "nullable")]
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
pub struct ItemEntry {
    pub item: Item,
    pub vendors: Vec<Seller>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct IndexData {
    pub fetched_at: String,
    #[serde(deserialize_with = "nullable")]
    pub published_at: Option<String>,
    pub record_count: usize,
    pub item_count: usize,
    pub items: OrderedMap<ItemEntry>,
    pub sync_id: Option<String>,
}
pub type Index = Payload<IndexData>;
#[derive(Clone, Debug, Deserialize)]
pub struct Attempt {
    pub attempted_at: String,
    pub requests: u64,
    pub sync_id: Option<String>,
    pub spends: Vec<Spend>,
    #[serde(default)]
    pub failed: bool,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub finished_at: Option<String>,
    pub last_request_at: Option<String>,
    #[serde(default)]
    pub total_requests: u64,
}
#[derive(Deserialize)]
struct PreviousAttempt {
    sync_id: Option<String>,
    spends: Vec<Spend>,
    #[serde(default)]
    failed: bool,
    status: Option<u16>,
    error: Option<String>,
    finished_at: Option<String>,
    #[serde(default)]
    total_requests: u64,
}
#[derive(Deserialize)]
struct UnfinishedAttempt {
    requests: u64,
    last_request_at: Option<String>,
    attempted_at: Option<String>,
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
    #[serde(deserialize_with = "nullable")]
    next_offset: Option<usize>,
    vendor_ids: Vec<String>,
    details: OrderedMap<Detail>,
    #[serde(deserialize_with = "nullable")]
    metadata: Option<Metadata>,
    total_requests: u64,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Deferred {
    #[serde(rename = "daily sync limit")]
    DailyLimit,
    #[serde(rename = "retry wait")]
    RetryWait,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SyncData {
    pub configured: bool,
    pub complete: bool,
    pub pending: bool,
    pub requests: u64,
    pub total_requests: u64,
    pub failed: bool,
    pub deferred: Option<Deferred>,
    pub status: Option<u16>,
    pub error: Option<String>,
}
pub type SyncResult = Payload<SyncData>;
#[derive(Clone, Debug, Deserialize)]
pub struct AttemptSummary {
    pub attempted_at: Option<String>,
    #[serde(default)]
    pub requests: u64,
    #[serde(default)]
    pub failed: bool,
    pub status: Option<u16>,
}
pub fn cache_dir(context: &Context<'_>) -> PathBuf {
    context.config.data_dir.join("vendors").join(BACKEND)
}
pub(crate) enum CacheError {
    Invalid(Failure),
    Failure(Failure),
    Io(std::io::Error),
}
impl CacheError {
    pub(crate) fn into_failure(self) -> Failure {
        match self {
            Self::Invalid(error) | Self::Failure(error) => error,
            Self::Io(error) => error.into(),
        }
    }
}
impl From<Failure> for CacheError {
    fn from(error: Failure) -> Self {
        Self::Invalid(error)
    }
}
impl From<std::io::Error> for CacheError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
pub(crate) type CacheResult<T> = std::result::Result<T, CacheError>;
pub fn read(context: &Context<'_>, name: &str) -> Result<Option<Document>> {
    cached_read(context, name).map_err(CacheError::into_failure)
}
fn cached_read(context: &Context<'_>, name: &str) -> CacheResult<Option<Document>> {
    read_at(&cache_dir(context), name)
}
fn read_at(dir: &Path, name: &str) -> CacheResult<Option<Document>> {
    let bytes = match std::fs::read(dir.join(name)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let text = String::from_utf8(bytes).map_err(|e| Failure::shape("ForeverDB cache text", e))?;
    let doc = crate::document::parse(&text)?;
    Ok((!matches!(doc, Document::Null)).then_some(doc))
}
fn sync_read(context: &Context<'_>, name: &str) -> SyncRun<Option<Document>> {
    read_at(&cache_dir(context), name).map_err(|error| match error {
        CacheError::Io(error) => SyncFailure::Failed(sync_io(error)),
        CacheError::Invalid(error) => SyncFailure::InvalidCache(error),
        CacheError::Failure(error) => SyncFailure::Failed(error),
    })
}
enum SyncFailure {
    InvalidCache(Failure),
    Failed(Failure),
}
impl From<Failure> for SyncFailure {
    fn from(error: Failure) -> Self {
        Self::Failed(error)
    }
}
type SyncRun<T> = std::result::Result<T, SyncFailure>;
fn load_attempt(context: &Context<'_>) -> SyncRun<Option<Document>> {
    let Some(mut data) = sync_read(context, "attempt.json")? else {
        return Ok(None);
    };
    cache_object(&data, "ForeverDB attempt").map_err(SyncFailure::InvalidCache)?;
    let attempt: PreviousAttempt = data
        .decode("ForeverDB attempt")
        .map_err(SyncFailure::InvalidCache)?;
    if attempt.finished_at.as_deref().is_none_or(str::is_empty) {
        let unfinished: UnfinishedAttempt = data
            .decode("ForeverDB unfinished attempt")
            .map_err(SyncFailure::InvalidCache)?;
        let finished = unfinished
            .last_request_at
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(unfinished.attempted_at.as_deref())
            .ok_or_else(|| {
                SyncFailure::InvalidCache(Failure::shape(
                    "ForeverDB unfinished attempt",
                    "missing attempted_at",
                ))
            })?;
        let mut spends = data
            .get("spends")
            .and_then(Document::as_array)
            .ok_or_else(|| Failure::shape("ForeverDB spends", "expected array"))?
            .to_vec();
        spends.push(spend_document(unfinished.requests, finished)?);
        data.set("spends", Document::Array(spends))?;
    }
    prune_spends(&mut data, context.clock).map_err(SyncFailure::InvalidCache)?;
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
pub fn attempt(context: &Context<'_>) -> Result<Option<AttemptSummary>> {
    cached_attempt(context).map_err(CacheError::into_failure)
}
fn cached_attempt(context: &Context<'_>) -> CacheResult<Option<AttemptSummary>> {
    cached_read(context, "attempt.json")?
        .filter(Document::truthy)
        .map(|d| {
            cache_object(&d, "ForeverDB attempt")?;
            Ok(d.decode("ForeverDB attempt")?)
        })
        .transpose()
}
fn random_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .map_err(sync_io)?
        .read_exact(&mut bytes)
        .map_err(sync_io)?;
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
            .open(&path)
            .map_err(sync_io)?;
        stream
            .write_all(dumps(&doc, JsonMode::Stdout).as_bytes())
            .map_err(sync_io)?;
        stream.sync_all().map_err(sync_io)?;
        std::fs::rename(&path, dir.join(name)).map_err(sync_io)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(path);
    result
}
fn remove(dir: &Path, name: &str) -> Result<()> {
    match std::fs::remove_file(dir.join(name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(sync_io(e)),
    }
}
pub fn index(context: &Context<'_>) -> Result<Option<Index>> {
    cached_index(context).map_err(CacheError::into_failure)
}
fn cached_index(context: &Context<'_>) -> CacheResult<Option<Index>> {
    if transport::Transport::select(context.config)
        .map_err(CacheError::Failure)?
        .is_none()
    {
        return Ok(None);
    }
    Ok(decode_index(cached_read(context, "index.json")?)?)
}
pub(crate) fn cache_object(doc: &Document, boundary: &str) -> Result<()> {
    if matches!(doc, Document::Object(_)) {
        Ok(())
    } else {
        Err(Failure::shape(boundary, "expected object"))
    }
}
fn matching_format(doc: &Document) -> Result<bool> {
    cache_object(doc, "ForeverDB cache")?;
    Ok(matches!(doc.get("format"), Some(Document::Number(n)) if *n == Number::Small(FORMAT)))
}
fn decode_index(doc: Option<Document>) -> Result<Option<Index>> {
    let Some(doc) = doc.filter(Document::truthy) else {
        return Ok(None);
    };
    if !matching_format(&doc)? {
        return Ok(None);
    }
    Payload::decode(doc, "ForeverDB index").map(Some)
}
pub fn usable_index(
    index: &Index,
    attempt: Option<&AttemptSummary>,
    context: &Context<'_>,
) -> Result<bool> {
    match attempt.filter(|a| a.failed) {
        Some(a) => Ok(index_age(index, context)? <= 7 * 86400 && a.status.is_none_or(|s| s >= 500)),
        None => Ok(true),
    }
}
fn index_age(index: &Index, context: &Context<'_>) -> Result<i64> {
    age(Some(&index.view().fetched_at), context.clock)
        .ok_or_else(|| Failure::shape("ForeverDB index", "invalid fetched_at"))
}
pub fn active_index(context: &Context<'_>) -> Result<Option<Index>> {
    cached_active_index(context).map_err(CacheError::into_failure)
}
pub(crate) fn cached_active_index(context: &Context<'_>) -> CacheResult<Option<Index>> {
    let Some(index) = cached_index(context)? else {
        return Ok(None);
    };
    let attempt = cached_attempt(context)?;
    Ok(usable_index(&index, attempt.as_ref(), context)?.then_some(index))
}
pub fn sync(context: &Context<'_>) -> Result<SyncResult> {
    let mut result = SyncData::default();
    let run = (|| {
        let Some(transport) = transport::Transport::select(context.config)? else {
            return Ok(());
        };
        result.configured = true;
        let dir = cache_dir(context);
        std::fs::create_dir_all(&dir).map_err(sync_io)?;
        let lock = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("sync.lock"))
            .map_err(sync_io)?;
        lock.lock().map_err(sync_io)?;
        sync_locked(context, &transport, &dir, &mut result)
    })();
    match run {
        Ok(()) => {}
        Err(SyncFailure::InvalidCache(error)) => return Err(error),
        Err(SyncFailure::Failed(error)) => {
            result.configured = true;
            result.failed = true;
            result.pending = cache_dir(context).join("pending.json").exists();
            result.error = Some(error.message);
        }
    }
    Payload::decode(Document::from_typed(&result)?, "ForeverDB sync")
}
fn sync_locked(
    context: &Context<'_>,
    transport: &transport::Transport,
    dir: &Path,
    result: &mut SyncData,
) -> SyncRun<()> {
    let SyncPlan::Attempt(plan) = plan_sync(context, dir, result)? else {
        return Ok(());
    };
    let PlannedAttempt {
        previous,
        mut pending,
        mut attempt,
        budget,
    } = *plan;
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
        result,
        deferred: None,
    };
    engine.checkpoint_interrupt()?;
    let mut obsolete = false;
    let outcome = (|| {
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
                return Err(mapping::invalid().into());
            }
            if page
                .ids
                .iter()
                .any(|id| engine.pending.view().vendor_ids.contains(id))
            {
                return Err(mapping::invalid().into());
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
            engine.attempt.set("error", Document::Null)?;
        } else {
            interrupt.write("pending.json", engine.pending.document(), context.clock)?;
        }
        engine.finish()?;
        interrupt.write("attempt.json", engine.attempt.document(), context.clock)?;
        engine.result.complete = complete;
        engine.result.pending = !complete;
        engine.result.deferred = engine.deferred;
        Ok(())
    })();
    if let Err(SyncFailure::InvalidCache(error)) = outcome {
        interrupt.clear();
        return Err(SyncFailure::InvalidCache(error));
    }
    if let Err(SyncFailure::Failed(e)) = &outcome {
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
        engine.attempt.set("error", e.message.clone())?;
        interrupt.write("attempt.json", engine.attempt.document(), context.clock)?;
        if obsolete {
            interrupt.remove("pending.json")?;
        } else {
            interrupt.write("pending.json", engine.pending.document(), context.clock)?;
        }
        engine.result.failed = true;
        engine.result.pending = !obsolete;
        engine.result.status = engine.attempt.view().status;
        engine.result.error = engine.attempt.view().error.clone();
    }
    interrupt.clear();
    Ok(())
}
enum SyncPlan {
    BudgetSpent,
    Attempt(Box<PlannedAttempt>),
}
struct PlannedAttempt {
    previous: Option<Index>,
    pending: Payload<Pending>,
    attempt: Payload<Attempt>,
    budget: RequestBudget,
}
#[derive(Deserialize)]
struct CheckpointTotal {
    #[serde(default)]
    total_requests: u64,
}
#[derive(Deserialize)]
struct CheckpointIdentity {
    sync_id: String,
}
fn plan_sync(context: &Context<'_>, dir: &Path, result: &mut SyncData) -> SyncRun<SyncPlan> {
    let previous =
        decode_index(sync_read(context, "index.json")?).map_err(SyncFailure::InvalidCache)?;
    let old_record = load_attempt(context)?;
    let old_attempt: Option<PreviousAttempt> = old_record
        .as_ref()
        .map(|d| d.decode("ForeverDB attempt"))
        .transpose()?;
    let pending = sync_read(context, "pending.json")?;
    let budget = RequestBudget::remaining(
        context.config.foreverdb_daily_requests,
        old_attempt.as_ref().map_or(&[], |attempt| &attempt.spends),
    );
    if budget.0 == 0 {
        result.configured = true;
        result.complete = previous.is_some() && pending.is_none();
        result.pending = pending.is_some();
        result.total_requests = match pending.as_ref().filter(|d| d.truthy()) {
            Some(doc) => {
                cache_object(doc, "ForeverDB checkpoint total")
                    .map_err(SyncFailure::InvalidCache)?;
                doc.decode::<CheckpointTotal>("ForeverDB checkpoint total")
                    .map_err(SyncFailure::InvalidCache)?
                    .total_requests
            }
            None => old_attempt.as_ref().map_or(0, |a| a.total_requests),
        };
        result.deferred = Some(Deferred::DailyLimit);
        result.failed = old_attempt.as_ref().is_some_and(|a| a.failed);
        result.status = old_attempt.as_ref().and_then(|a| a.status);
        result.error = old_attempt.as_ref().and_then(|a| a.error.clone());
        return Ok(SyncPlan::BudgetSpent);
    }

    let pending = prepare_checkpoint(
        context,
        dir,
        pending,
        previous.as_ref(),
        old_attempt.as_ref(),
    )?;
    let attempt: Payload<Attempt> = Payload::decode(
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
    result.total_requests = pending.view().total_requests;
    Ok(SyncPlan::Attempt(Box::new(PlannedAttempt {
        previous,
        pending,
        attempt,
        budget,
    })))
}
fn prepare_checkpoint(
    context: &Context<'_>,
    dir: &Path,
    mut pending: Option<Document>,
    previous: Option<&Index>,
    old_attempt: Option<&PreviousAttempt>,
) -> SyncRun<Payload<Pending>> {
    if let Some(doc) = pending.as_ref().filter(|d| d.truthy())
        && !matching_format(doc).map_err(SyncFailure::InvalidCache)?
    {
        pending = None;
    }
    if let (Some(p), Some(old)) = (pending.as_ref().filter(|d| d.truthy()), previous) {
        let identity: CheckpointIdentity = p
            .decode("ForeverDB checkpoint identity")
            .map_err(SyncFailure::InvalidCache)?;
        if Some(identity.sync_id.as_str()) == old.view().sync_id.as_deref() {
            remove(dir, "pending.json")?;
            pending = None;
        }
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
    let mut pending: Payload<Pending> =
        Payload::decode(pending, "ForeverDB checkpoint").map_err(SyncFailure::InvalidCache)?;
    if pending.view().next_offset.is_none() && pending.view().metadata.is_none() {
        return Err(SyncFailure::InvalidCache(Failure::shape(
            "ForeverDB checkpoint",
            "metadata is required after the vendor pages are complete",
        )));
    }
    if let Some(a) = old_attempt
        && a.sync_id.as_deref() == Some(&pending.view().sync_id)
    {
        let total = pending.view().total_requests.max(a.total_requests);
        pending.set("total_requests", Document::from_typed(&total)?)?;
    }
    Ok(pending)
}
struct Engine<'a, 'b> {
    context: &'a Context<'b>,
    transport: &'a transport::Transport,
    pending: &'a mut Payload<Pending>,
    attempt: &'a mut Payload<Attempt>,
    budget: RequestBudget,
    last_start: Option<f64>,
    interrupt: &'a interrupt::Guard,
    result: &'a mut SyncData,
    deferred: Option<Deferred>,
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
    fn get(&mut self, path: &str) -> SyncRun<Option<Document>> {
        loop {
            if self.attempt.view().requests >= self.budget.0 {
                self.deferred = Some(Deferred::DailyLimit);
                return Ok(None);
            }
            let retry = self
                .pending
                .document()
                .get("retry")
                .filter(|d| d.truthy())
                .map(|d| {
                    cache_object(d, "ForeverDB retry").map_err(SyncFailure::InvalidCache)?;
                    d.decode::<Retry>("ForeverDB retry")
                        .map_err(SyncFailure::InvalidCache)
                })
                .transpose()?;
            self.checkpoint_interrupt()?;
            if let Some(retry) = &retry {
                let delay = (instant(&retry.not_before)
                    .map_err(SyncFailure::InvalidCache)?
                    .microseconds()
                    - self.context.clock.now().microseconds()) as f64
                    / 1e6;
                if delay > 300.0 {
                    self.deferred = Some(Deferred::RetryWait);
                    return Ok(None);
                }
                if delay > 0.0 {
                    self.context.clock.sleep(delay.clamp(60.0, 300.0))?;
                }
            }
            if let Some(last) = self.last_start {
                self.context
                    .clock
                    .sleep((INTERVAL - (self.context.clock.monotonic() - last)).max(0.0))?;
            }
            let mut reservation = self.attempt.clone();
            let requests = self
                .attempt
                .view()
                .requests
                .checked_add(1)
                .ok_or_else(mapping::invalid)?;
            let total = self
                .pending
                .view()
                .total_requests
                .checked_add(1)
                .ok_or_else(mapping::invalid)?;
            reservation.set("requests", Document::from_typed(&requests)?)?;
            reservation.set("total_requests", Document::from_typed(&total)?)?;
            reservation.set("last_request_at", iso(self.context.clock.now()))?;
            self.interrupt
                .write("attempt.json", reservation.document(), self.context.clock)?;
            *self.attempt = reservation;
            self.pending
                .set("total_requests", Document::from_typed(&total)?)?;
            self.result.requests = requests;
            self.result.total_requests = total;
            self.last_start = Some(self.context.clock.monotonic());
            self.checkpoint_interrupt()?;
            match self.transport.request(path, self.context) {
                Ok(raw) => {
                    self.interrupt
                        .clear_retry(self.pending, self.context.clock)?;
                    return crate::http::decode_body(&raw)
                        .map(Some)
                        .map_err(|_| SyncFailure::Failed(mapping::invalid()));
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
                        let timestamp = self.context.clock.now();
                        let not_before =
                            retry_at(response.and_then(|d| d.get("retry_after")), timestamp)?;
                        self.pending.set(
                            "retry",
                            Document::object([("not_before", retry_iso(not_before).into())]),
                        )?;
                        self.interrupt.write(
                            "pending.json",
                            self.pending.document(),
                            self.context.clock,
                        )?;
                        if not_before.microseconds() - timestamp.microseconds() > 300_000_000 {
                            self.deferred = Some(Deferred::RetryWait);
                            return Ok(None);
                        }
                        continue;
                    }
                    self.interrupt
                        .clear_retry(self.pending, self.context.clock)?;
                    return Err(Failure {
                        message: crate::human::refresh::sync_failed(status),
                        code: crate::ExitCode::Unreachable,
                        body: Some(Document::object([("status", status.into())])),
                    }
                    .into());
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
    cached_index_sources(index, context).map_err(CacheError::into_failure)
}
pub(crate) fn cached_index_sources(
    index: &Index,
    context: &Context<'_>,
) -> CacheResult<Vec<Source>> {
    let rows = index
        .view()
        .items
        .0
        .iter()
        .flat_map(|(_, e)| e.vendors.clone())
        .collect::<Vec<_>>();
    Ok(provenance(
        index,
        &rows,
        cached_stale(index, context)?,
        context,
    )?)
}
fn cached_stale(index: &Index, context: &Context<'_>) -> CacheResult<bool> {
    Ok(index_age(index, context)? > 86400 || cached_attempt(context)?.is_some_and(|a| a.failed))
}
pub fn lookup(item_id: i64, index: &Index, context: &Context<'_>) -> Result<Document> {
    let key = item_id.to_string();
    let entry = index.view().items.get(&key);
    let docs = index.document().get("items").and_then(|d| d.get(&key));
    let stale = cached_stale(index, context).map_err(CacheError::into_failure)?;
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
