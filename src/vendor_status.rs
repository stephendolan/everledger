use crate::{
    Context, Result,
    document::Document,
    http::Payload,
    provenance::{Source, source},
};
use serde::Deserialize;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Backend {
    #[serde(rename = "wowforever.ninja")]
    Ninja,
    #[serde(rename = "foreverdb.net")]
    Foreverdb,
}
impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ninja => super::BACKEND,
            Self::Foreverdb => crate::foreverdb::BACKEND,
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize)]
pub enum ForeverdbState {
    #[serde(rename = "not configured")]
    NotConfigured,
    #[serde(rename = "synced")]
    Synced,
    #[serde(rename = "awaiting completed sync")]
    Awaiting,
    #[serde(rename = "sync failed; serving previous index")]
    ServingPrevious,
    #[serde(rename = "sync failed; ninja fallback")]
    NinjaFallback,
}
impl ForeverdbState {
    pub fn name(self) -> &'static str {
        match self {
            Self::NotConfigured => "not configured",
            Self::Synced => "synced",
            Self::Awaiting => "awaiting completed sync",
            Self::ServingPrevious => "sync failed; serving previous index",
            Self::NinjaFallback => "sync failed; ninja fallback",
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize)]
pub enum TransportName {
    #[serde(rename = "env-key")]
    EnvKey,
    #[serde(rename = "connector-surrogate")]
    ConnectorSurrogate,
}
impl TransportName {
    pub fn name(self) -> &'static str {
        match self {
            Self::EnvKey => "env-key",
            Self::ConnectorSurrogate => "connector-surrogate",
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct ForeverdbInfo {
    pub state: ForeverdbState,
    pub transport: Option<TransportName>,
    pub last_sync_at: Option<String>,
    pub record_count: usize,
    pub item_count: usize,
    pub pending: bool,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Counts {
    pub record_count: usize,
    pub item_count: usize,
}
impl Counts {
    fn document(&self) -> Document {
        Document::object([
            ("record_count", self.record_count.into()),
            ("item_count", self.item_count.into()),
        ])
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct Coverage {
    #[serde(rename = "wowforever.ninja")]
    pub ninja: Counts,
    #[serde(rename = "foreverdb.net")]
    pub foreverdb: Counts,
}
#[derive(Clone, Debug, Deserialize)]
pub struct BackendStatusData {
    pub backend: Backend,
    pub cache_items: usize,
    pub published_at: Option<String>,
    pub seen: usize,
    pub baseline: usize,
    pub unverified: usize,
    pub foreverdb: ForeverdbInfo,
    pub coverage: Coverage,
}
pub type BackendStatus = Payload<BackendStatusData>;
pub fn backend_status(context: &Context<'_>) -> Result<(BackendStatus, Vec<Source>)> {
    let transport = crate::foreverdb::transport::Transport::select(context.config)?;
    let configured = transport.is_some();
    let completed = if configured {
        crate::foreverdb::index(context)?
    } else {
        None
    };
    let attempt = if configured {
        crate::foreverdb::attempt(context)?
    } else {
        None
    };
    let active = match &completed {
        Some(index) if crate::foreverdb::usable_index(index, attempt.as_ref(), context)? => {
            Some(index)
        }
        Some(_) | None => None,
    };
    let state = if attempt.as_ref().is_some_and(|a| a.failed) {
        if active.is_some() {
            ForeverdbState::ServingPrevious
        } else {
            ForeverdbState::NinjaFallback
        }
    } else if !configured {
        ForeverdbState::NotConfigured
    } else if active.is_some() {
        ForeverdbState::Synced
    } else {
        ForeverdbState::Awaiting
    };
    let info = Document::object([
        ("backend", crate::foreverdb::BACKEND.into()),
        ("state", state.name().into()),
        ("transport", transport.as_ref().map(|t| t.name()).into()),
        (
            "last_sync_at",
            completed
                .as_ref()
                .map(|i| i.view().fetched_at.clone())
                .into(),
        ),
        (
            "record_count",
            completed
                .as_ref()
                .map_or(0, |i| i.view().record_count)
                .into(),
        ),
        (
            "item_count",
            completed.as_ref().map_or(0, |i| i.view().item_count).into(),
        ),
        (
            "last_attempt_at",
            attempt.as_ref().and_then(|a| a.attempted_at.clone()).into(),
        ),
        (
            "last_attempt_requests",
            Document::from_typed(&attempt.as_ref().map_or(0, |a| a.requests))?,
        ),
        (
            "pending",
            (configured
                && crate::foreverdb::cache_dir(context)
                    .join("pending.json")
                    .exists())
            .into(),
        ),
    ]);
    let mut cached = vec![];
    for path in super::cache_paths(context)? {
        let document = crate::document::parse(&std::fs::read_to_string(path)?)?;
        if super::cache_matches(&document, context)? {
            cached.push(document.decode::<super::CachedCoverage>("ninja status cache")?);
        }
    }
    let ninja_rows = cached
        .iter()
        .flat_map(|c| c.vendors.iter().flatten())
        .collect::<Vec<_>>();
    let ninja = Counts {
        record_count: ninja_rows
            .iter()
            .map(|r| r.npc_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        item_count: cached.len(),
    };
    let foreverdb = Counts {
        record_count: completed.as_ref().map_or(0, |i| i.view().record_count),
        item_count: completed.as_ref().map_or(0, |i| i.view().item_count),
    };
    let coverage = Document::object([
        (super::BACKEND, ninja.document()),
        (crate::foreverdb::BACKEND, foreverdb.document()),
    ]);
    let (summary, sources) = match active {
        Some(index) => {
            let mut counts = [0usize; 3];
            for row in index.view().items.0.iter().flat_map(|(_, e)| &e.vendors) {
                match row.basis {
                    crate::foreverdb::mapping::Basis::Seen => counts[0] += 1,
                    crate::foreverdb::mapping::Basis::Baseline => counts[1] += 1,
                    crate::foreverdb::mapping::Basis::Unverified => counts[2] += 1,
                }
            }
            let summary = Document::object([
                ("source", "vendors".into()),
                ("backend", crate::foreverdb::BACKEND.into()),
                ("state", state.name().into()),
                ("cache_items", index.view().item_count.into()),
                ("published_at", index.view().published_at.as_deref().into()),
                ("last_sync_at", index.view().fetched_at.clone().into()),
                ("record_count", index.view().record_count.into()),
                ("seen", counts[0].into()),
                ("baseline", counts[1].into()),
                ("unverified", counts[2].into()),
                ("foreverdb", info),
                ("coverage", coverage),
            ]);
            (summary, crate::foreverdb::index_sources(index, context)?)
        }
        None => {
            let mut counts = [0usize; 3];
            for row in ninja_rows {
                match row.basis {
                    super::Basis::Seen => counts[0] += 1,
                    super::Basis::Baseline => counts[1] += 1,
                    super::Basis::Unverified => counts[2] += 1,
                }
            }
            let published = cached
                .iter()
                .filter_map(|c| c.published_at.as_deref().filter(|s| !s.is_empty()))
                .max();
            let summary = Document::object([
                ("source", "vendors".into()),
                ("backend", super::BACKEND.into()),
                ("cache_items", cached.len().into()),
                ("published_at", published.into()),
                ("seen", counts[0].into()),
                ("baseline", counts[1].into()),
                ("unverified", counts[2].into()),
                ("foreverdb", info),
                ("coverage", coverage),
            ]);
            let mut s = source(super::BACKEND, published, context.clock)?;
            s.set(
                "detail",
                crate::human::status::ninja_detail(cached.len(), state),
            )?;
            (summary, vec![s])
        }
    };
    Ok((Payload::decode(summary, "vendor backend status")?, sources))
}
