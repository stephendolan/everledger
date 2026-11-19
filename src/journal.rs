use crate::{
    Context, Copper, EpochSeconds, Failure, Result,
    catalogue::{Item, ItemRef, MatchBasis, RecordedItem, item_basis, item_matches},
    document::{Document, JsonMode, dumps},
    http::Payload,
    primitives::{base64_decode, base64_encode, casefold},
    provenance::{Field, Source, sources},
    roster::{character_path, roster},
};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Event {
    pub item: Option<RecordedItem>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub venue: Option<String>,
    pub character: Option<String>,
    pub count: Option<i64>,
    pub cash_copper: Option<Copper>,
    pub proceeds_copper: Option<Copper>,
    pub recorded_at: Option<String>,
    pub match_basis: Option<String>,
}
impl Event {
    pub fn actor(&self) -> Option<&str> {
        self.character.as_deref().filter(|s| !s.is_empty())
    }
    pub fn item(&self) -> ItemRef<'_> {
        self.item
            .as_ref()
            .map(RecordedItem::reference)
            .unwrap_or_default()
    }
    pub fn matched_by_name(&self) -> bool {
        self.match_basis.as_deref() == Some("name") || self.item().matched_by_name()
    }
}
pub type EventRow = Payload<Event>;
#[derive(Clone, Debug, Deserialize)]
pub struct JournalState {
    pub has_more: Option<bool>,
    pub next_cursor: Option<String>,
    pub checkpoint: Option<String>,
    pub after_cursor: Option<String>,
    pub available: Option<bool>,
    pub state: Option<String>,
    pub reason: Option<String>,
}
impl JournalState {
    pub fn unavailable(&self) -> bool {
        self.available == Some(false)
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct EventData {
    #[serde(rename = "entries")]
    _entries: Option<Vec<Event>>,
    #[serde(flatten)]
    pub state: JournalState,
}
pub type Journal = Payload<EventData>;
impl Journal {
    pub fn rows(&self) -> Result<Option<Vec<EventRow>>> {
        self.document()
            .get("entries")
            .map(|_| row_payloads(self.document(), "entries", "journal event"))
            .transpose()
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct AggregateData {
    #[serde(rename = "events")]
    _events: Vec<Event>,
    #[serde(flatten)]
    pub state: JournalState,
}
pub type Aggregate = Payload<AggregateData>;
impl Aggregate {
    pub fn rows(&self) -> Result<Vec<EventRow>> {
        row_payloads(self.document(), "events", "journal aggregate event")
    }
}
#[derive(Clone, Debug)]
pub struct ItemView(Payload<Item>);
impl ItemView {
    pub fn resolved(item: &Item) -> Result<Self> {
        Ok(Self(Payload::decode(
            Document::from_typed(item)?,
            "journal item view",
        )?))
    }
}

#[derive(Clone, Debug, Default)]
pub struct View {
    pub char: Option<String>,
    pub item: Option<ItemView>,
    pub venue: Option<String>,
    pub event_type: Option<String>,
}
impl View {
    fn document(&self) -> Result<Document> {
        let mut entries = vec![];
        if let Some(v) = &self.char {
            entries.push(("char", Document::from(v.clone())));
        }
        if let Some(v) = &self.item {
            entries.push(("item", v.0.document().clone()));
        }
        if let Some(v) = &self.venue {
            entries.push(("venue", Document::from(v.clone())));
        }
        if let Some(v) = &self.event_type {
            entries.push(("type", Document::from(v.clone())));
        }
        Ok(Document::object(entries))
    }
    fn includes(&self, event: &Event) -> bool {
        for (filter, value) in [
            (&self.event_type, Some(event.event_type.as_str())),
            (&self.venue, event.venue.as_deref()),
        ] {
            if let Some(filter) = filter.as_deref().filter(|s| !s.is_empty())
                && !filter.split(',').any(|s| Some(s) == value)
            {
                return false;
            }
        }
        if let Some(name) = self.char.as_deref().filter(|s| !s.is_empty())
            && casefold(event.actor().unwrap_or("")) != casefold(name)
        {
            return false;
        }
        self.item
            .as_ref()
            .is_none_or(|item| item_matches(event.item(), item.0.view()))
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum TimeBound {
    Epoch(EpochSeconds),
    Iso(String),
}
impl TimeBound {
    fn document(&self) -> Document {
        match self {
            Self::Epoch(n) => Document::from(n.0),
            Self::Iso(s) => Document::from(s.clone()),
        }
    }
}
#[derive(Clone, Debug)]
pub struct WatchPosition(String);
#[derive(Clone, Debug)]
pub struct ServicePage(String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageCursor(String);
impl PageCursor {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Copy, Debug)]
pub enum WindowOrder {
    SinceUntil,
    UntilSince,
}
#[derive(Clone, Copy, Debug)]
pub enum PageOrder {
    CursorLimit,
    LimitCursor,
}
#[derive(Clone, Debug)]
pub enum Query {
    Window {
        since: Option<TimeBound>,
        until: Option<TimeBound>,
        order: WindowOrder,
    },
    Watch(WatchPosition),
    Page {
        cursor: ServicePage,
        until: Option<TimeBound>,
        order: PageOrder,
    },
}
#[derive(Clone, Debug)]
pub struct EventQuery {
    pub view: View,
    pub limit: Option<i64>,
    pub query: Query,
}
pub struct Selectors<'a> {
    pub character: Option<&'a str>,
    pub since: Option<&'a str>,
    pub until: Option<&'a str>,
    pub venue: Option<&'a str>,
    pub event_type: Option<&'a str>,
    pub after: Option<&'a str>,
    pub limit: Option<i64>,
    pub default_since: Option<&'a str>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Character {
    #[serde(default)]
    pub name: Field<String>,
    pub short_name: Option<String>,
}
#[derive(Deserialize)]
pub struct SelectedCharacter {
    pub character: Character,
}
pub fn event_params(s: Selectors<'_>, context: &Context<'_>) -> Result<EventQuery> {
    let event_type = s
        .event_type
        .map(|v| v.split(',').map(str::trim).collect::<Vec<_>>().join(","));
    if event_type.as_ref().is_some_and(|v| {
        v.split(',')
            .any(|t| !["purchase", "sale", "posting", "expiration", "cancellation"].contains(&t))
    }) {
        return Err(Failure::usage(
            "--type must contain: purchase, sale, posting, expiration, cancellation",
        ));
    }
    let character = if let Some(name) = s.character.filter(|s| !s.is_empty()) {
        let selected: Payload<SelectedCharacter> =
            roster(context.http, context.config, &character_path(name), &[])?;
        match &selected.view().character.name {
            Field::Missing => return Err(Failure::shape("character", "missing name")),
            Field::Null => None,
            Field::Value(name) => Some(name.clone()),
        }
    } else {
        s.character.map(str::to_owned)
    };
    let query = match s.after {
        Some("") => {
            return Err(Failure::usage(
                "empty --after; bootstrap with: everledger events --limit 0 --json",
            ));
        }
        Some(after) => {
            if s.since.is_some() || s.until.is_some() {
                return Err(Failure::usage(
                    "--after cannot be combined with --since or --until",
                ));
            }
            Query::Watch(WatchPosition(after.into()))
        }
        None => {
            let since = s
                .since
                .filter(|s| !s.is_empty())
                .or(s.default_since)
                .map(|v| crate::time::time_selector(v, context.clock))
                .transpose()?;
            let until = s
                .until
                .filter(|s| !s.is_empty())
                .map(|v| crate::time::time_selector(v, context.clock))
                .transpose()?;
            if let (Some(a), Some(b)) = (&since, &until)
                && a >= b
            {
                return Err(Failure::usage("--since must precede exclusive --until"));
            }
            Query::Window {
                since: since.map(TimeBound::Iso),
                until: until.map(TimeBound::Iso),
                order: WindowOrder::SinceUntil,
            }
        }
    };
    Ok(EventQuery {
        view: View {
            char: character,
            item: None,
            venue: s.venue.map(str::to_owned),
            event_type,
        },
        limit: s.limit,
        query,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedView {
    char: Option<String>,
    item: Option<Item>,
    venue: Option<String>,
    #[serde(rename = "type")]
    event_type: Option<String>,
}
#[derive(Deserialize)]
struct EncodedPage {
    cursor: String,
    view: EncodedView,
    until: Option<TimeBound>,
}
pub fn page_params(cursor: &str, limit: Option<i64>, order: PageOrder) -> Result<EventQuery> {
    let invalid = || Failure::usage("invalid snapshot page cursor");
    let (cursor, view, until) = if let Some(encoded) = cursor.strip_prefix("elpage1:") {
        let bytes = base64_decode(encoded).map_err(|_| invalid())?;
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
        let document = crate::document::parse(text).map_err(|_| invalid())?;
        let decoded: EncodedPage = document
            .decode("snapshot page cursor")
            .map_err(|_| invalid())?;
        let view = document
            .get("view")
            .and_then(Document::as_object)
            .ok_or_else(invalid)?;
        if decoded.cursor.is_empty() || view.iter().any(|(_, v)| matches!(v, Document::Null)) {
            return Err(invalid());
        }
        let item = decoded
            .view
            .item
            .map(|_| -> Result<ItemView> {
                let item = document
                    .get("view")
                    .and_then(|v| v.get("item"))
                    .cloned()
                    .ok_or_else(invalid)?;
                Ok(ItemView(
                    Payload::decode(item, "snapshot item view").map_err(|_| invalid())?,
                ))
            })
            .transpose()?;
        let view = View {
            char: decoded.view.char,
            item,
            venue: decoded.view.venue,
            event_type: decoded.view.event_type,
        };
        (decoded.cursor, view, decoded.until)
    } else {
        (cursor.into(), View::default(), None)
    };
    Ok(EventQuery {
        query: Query::Page {
            cursor: ServicePage(cursor),
            until,
            order,
        },
        view,
        limit,
    })
}
fn page_cursor(cursor: &str, view: &View, until: Option<&TimeBound>) -> Result<PageCursor> {
    let view = view.document()?;
    if view.as_object().is_some_and(|v| v.is_empty()) && until.is_none() {
        return Ok(PageCursor(cursor.into()));
    }
    let doc = Document::object([
        ("cursor", Document::from(cursor)),
        ("view", view),
        ("until", until.map_or(Document::Null, TimeBound::document)),
    ]);
    Ok(PageCursor(format!(
        "elpage1:{}",
        base64_encode(dumps(&doc, JsonMode::Cursor).as_bytes())
    )))
}
impl EventQuery {
    fn wire(&self) -> Vec<(String, Document)> {
        let limit = ("limit".into(), Document::from(self.limit));
        match &self.query {
            Query::Window {
                since,
                until,
                order,
            } => {
                let since = (
                    "since".into(),
                    since.as_ref().map_or(Document::Null, TimeBound::document),
                );
                let until = (
                    "until".into(),
                    until.as_ref().map_or(Document::Null, TimeBound::document),
                );
                match order {
                    WindowOrder::SinceUntil => vec![limit, since, until],
                    WindowOrder::UntilSince => vec![limit, until, since],
                }
            }
            Query::Watch(after) => vec![limit, ("after".into(), Document::from(after.0.clone()))],
            Query::Page {
                cursor,
                order,
                until: _,
            } => {
                let cursor = ("cursor".into(), Document::from(cursor.0.clone()));
                match order {
                    PageOrder::CursorLimit => vec![cursor, limit],
                    PageOrder::LimitCursor => vec![limit, cursor],
                }
            }
        }
    }
    fn until(&self) -> Option<&TimeBound> {
        match &self.query {
            Query::Window {
                until,
                since: _,
                order: _,
            }
            | Query::Page {
                until,
                cursor: _,
                order: _,
            } => until.as_ref(),
            Query::Watch(_) => None,
        }
    }
    fn resync(&self, since: TimeBound) -> Self {
        let order = match self.query {
            Query::Window {
                order,
                since: _,
                until: _,
            } => order,
            Query::Watch(_) => WindowOrder::SinceUntil,
            Query::Page {
                cursor: _,
                until: _,
                order: _,
            } => WindowOrder::UntilSince,
        };
        Self {
            view: self.view.clone(),
            limit: self.limit,
            query: Query::Window {
                since: Some(since),
                until: self.until().cloned(),
                order,
            },
        }
    }
}
#[derive(Clone, Debug)]
pub enum PageEnd {
    More(PageCursor),
    Complete(Option<WatchPosition>),
}
pub struct EventPage {
    pub data: Journal,
    pub end: PageEnd,
}
pub fn row_payloads<T: serde::de::DeserializeOwned>(
    document: &Document,
    key: &str,
    boundary: &str,
) -> Result<Vec<Payload<T>>> {
    match document.get(key) {
        None => Ok(vec![]),
        Some(Document::Array(rows)) => rows
            .iter()
            .map(|r| Payload::decode(r.clone(), boundary))
            .collect(),
        Some(_) => Err(Failure::shape(boundary, "expected array")),
    }
}
pub fn event_page(params: &EventQuery, context: &Context<'_>) -> Result<EventPage> {
    let mut data: Journal = roster(context.http, context.config, "/events", &params.wire())?;
    if let Some(rows) = data.rows()? {
        data.set(
            "entries",
            Document::Array(
                rows.into_iter()
                    .filter(|r| params.view.includes(r.view()))
                    .map(Payload::into_document)
                    .collect(),
            ),
        )?;
    }
    let end = if data.view().state.has_more == Some(true) {
        let cursor = data
            .view()
            .state
            .next_cursor
            .as_deref()
            .filter(|s| !s.is_empty());
        let cursor = pagination_progress(cursor, &[])?;
        let cursor = page_cursor(cursor, &params.view, params.until())?;
        data.set("next_cursor", cursor.as_str())?;
        data.set("after_cursor", Document::Null)?;
        PageEnd::More(cursor)
    } else {
        let checkpoint = data.view().state.checkpoint.clone();
        data.set("next_cursor", Document::Null)?;
        data.set("after_cursor", Document::from(checkpoint.clone()))?;
        PageEnd::Complete(checkpoint.map(WatchPosition))
    };
    Ok(EventPage { data, end })
}

fn pagination_progress<T: PartialEq>(next: Option<T>, seen: &[T]) -> Result<T> {
    next.filter(|cursor| !seen.contains(cursor)).ok_or_else(|| {
        Failure::error("events pagination did not advance; refusing a partial tally")
    })
}
#[derive(Deserialize)]
struct ErrorBody {
    status: i64,
    response: Option<JournalError>,
}
#[derive(Deserialize)]
struct JournalError {
    error_code: Option<String>,
    resync_since: Option<TimeBound>,
}
#[derive(Clone, Debug)]
enum Recovery {
    Resync(TimeBound),
    Restart,
    Refuse,
}
fn recovery(error: &Failure) -> Recovery {
    let body = error
        .body
        .as_ref()
        .and_then(|d| d.decode::<ErrorBody>("journal error").ok());
    if let Some(ErrorBody {
        status: 409,
        response: Some(body),
    }) = body
    {
        match (body.error_code.as_deref(), body.resync_since) {
            (Some("cursor_invalid"), Some(hint)) => return Recovery::Resync(hint),
            (Some("snapshot_changed"), _) => return Recovery::Restart,
            (Some("cursor_invalid"), None) | (None, _) | (Some(_), _) => {}
        }
    }
    Recovery::Refuse
}
fn announce(hint: &TimeBound, context: &Context<'_>) -> Result<()> {
    context
        .diagnostics
        .warn(&crate::human::events::resync_notice(hint))
}
#[derive(Clone, Copy)]
enum RetryScope {
    Page,
    Snapshot,
}
fn retry_once<T>(
    params: EventQuery,
    scope: RetryScope,
    context: &Context<'_>,
    operation: impl Fn(&EventQuery) -> Result<T>,
) -> Result<T> {
    match operation(&params) {
        Ok(data) => Ok(data),
        Err(error) => match recovery(&error) {
            Recovery::Resync(hint) => {
                announce(&hint, context)?;
                operation(&params.resync(hint))
            }
            Recovery::Restart if matches!(scope, RetryScope::Snapshot) => {
                context
                    .diagnostics
                    .warn(crate::human::events::SNAPSHOT_RESTART)?;
                operation(&params)
            }
            Recovery::Restart | Recovery::Refuse => Err(error),
        },
    }
}
pub fn fetch_events(params: EventQuery, context: &Context<'_>) -> Result<Journal> {
    if params.limit == Some(0) {
        let mut data = collect_events(params, true, context)?.into_document();
        data.remove("events");
        data.set("entries", Document::Array(vec![]))?;
        let journal: Journal = Payload::decode(data, "events")?;
        if !journal.view().state.unavailable()
            && journal
                .view()
                .state
                .checkpoint
                .as_deref()
                .is_none_or(str::is_empty)
        {
            return Err(Failure::error(
                "finished bootstrap has no service checkpoint",
            ));
        }
        Ok(journal)
    } else {
        retry_once(params, RetryScope::Page, context, |params| {
            Ok(event_page(params, context)?.data)
        })
    }
}
pub fn collect_events(
    params: EventQuery,
    discard: bool,
    context: &Context<'_>,
) -> Result<Aggregate> {
    retry_once(params, RetryScope::Snapshot, context, |params| {
        collect_snapshot(params, discard, context)
    })
}
fn collect_snapshot(
    params: &EventQuery,
    discard: bool,
    context: &Context<'_>,
) -> Result<Aggregate> {
    let mut first = params.clone();
    first.limit = Some(500);
    let mut page = event_page(&first, context)?;
    let mut rows = vec![];
    let mut used: Vec<Source> = vec![];
    let mut seen = vec![];
    loop {
        if !discard {
            rows.extend(
                page.data
                    .rows()?
                    .unwrap_or_default()
                    .into_iter()
                    .map(Payload::into_document),
            );
        }
        for entry in sources(page.data.document())? {
            if !used.iter().any(|s| s.document() == entry.document()) {
                used.push(entry);
            }
        }
        match page.end {
            PageEnd::Complete(_) => break,
            PageEnd::More(cursor) => {
                let cursor = pagination_progress(Some(cursor), &seen)?;
                let next = page_params(cursor.as_str(), Some(500), PageOrder::LimitCursor)?;
                seen.push(cursor);
                page = event_page(&next, context)?;
            }
        }
    }
    let mut doc = page.data.into_document();
    doc.remove("entries");
    doc.set("events", Document::Array(rows))?;
    doc.set("sources", source_documents(used))?;
    Payload::decode(doc, "journal aggregate")
}
pub fn source_documents(sources: Vec<Source>) -> Document {
    Document::Array(sources.into_iter().map(Payload::into_document).collect())
}
pub fn basis(row: ItemRef<'_>) -> &'static str {
    match item_basis(row) {
        MatchBasis::Id => "id",
        MatchBasis::Name => "name",
    }
}
pub fn item_events(
    mut params: EventQuery,
    item: Option<&Item>,
    context: &Context<'_>,
) -> Result<Aggregate> {
    params.view.item = item.map(ItemView::resolved).transpose()?;
    let mut data = collect_events(params, false, context)?;
    if item.is_some() {
        let rows = data
            .rows()?
            .into_iter()
            .map(|mut r| {
                r.set("match_basis", basis(r.view().item()))?;
                Ok(r.into_document())
            })
            .collect::<Result<Vec<_>>>()?;
        data.set("events", Document::Array(rows))?;
    }
    Ok(data)
}
pub fn count_total<'a>(rows: impl IntoIterator<Item = &'a Event>) -> Result<Option<i64>> {
    rows.into_iter()
        .try_fold(Some(0_i64), |sum, e| checked_count(sum, e.count))
}
pub fn checked_count(a: Option<i64>, b: Option<i64>) -> Result<Option<i64>> {
    match (a, b) {
        (Some(a), Some(b)) => a
            .checked_add(b)
            .map(Some)
            .ok_or_else(|| Failure::error("count out of range")),
        (None, _) | (_, None) => Ok(None),
    }
}
pub fn money_total(values: impl IntoIterator<Item = Option<Copper>>) -> Result<Option<Copper>> {
    values
        .into_iter()
        .try_fold(Some(Copper(0)), |sum, v| match (sum, v) {
            (Some(a), Some(b)) => {
                a.0.checked_add(b.0)
                    .map(|n| Some(Copper(n)))
                    .ok_or_else(|| Failure::error("copper total out of range"))
            }
            (None, _) | (_, None) => Ok(None),
        })
}
pub fn venue(v: Option<crate::types::Venue>) -> Option<&'static str> {
    v.map(|v| match v {
        crate::types::Venue::Auction => "auction",
        crate::types::Venue::Vendor => "vendor",
    })
}
