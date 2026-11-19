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
    let mut backend =
        super::BackendState::load(context).map_err(crate::foreverdb::CacheError::into_failure)?;
    let state = backend
        .state(context)
        .map_err(crate::foreverdb::CacheError::into_failure)?;
    let completed = backend.completed.as_ref().map(Payload::view);
    let transport = backend.transport.as_ref();
    let foreverdb = Counts {
        record_count: completed.map_or(0, |i| i.record_count),
        item_count: completed.map_or(0, |i| i.item_count),
    };
    let attempt = backend.attempt();
    let active = backend.serving_index(state);
    let info = Document::object([
        ("backend", crate::foreverdb::BACKEND.into()),
        ("state", state.name().into()),
        ("transport", transport.map(|t| t.name()).into()),
        (
            "last_sync_at",
            completed.map(|i| i.fetched_at.clone()).into(),
        ),
        ("record_count", foreverdb.record_count.into()),
        ("item_count", foreverdb.item_count.into()),
        (
            "last_attempt_at",
            attempt.and_then(|a| a.attempted_at.clone()).into(),
        ),
        (
            "last_attempt_requests",
            Document::from_typed(&attempt.map_or(0, |a| a.requests))?,
        ),
        (
            "pending",
            (transport.is_some()
                && crate::foreverdb::cache_dir(context)
                    .join("pending.json")
                    .exists())
            .into(),
        ),
    ]);
    let mut cached = vec![];
    for path in super::cache_paths(context)? {
        if let Some(document) = super::read_ninja_cache(&path, super::NinjaRead::Status, context)
            .map_err(crate::foreverdb::CacheError::into_failure)?
        {
            cached.push(document.decode::<super::CachedCoverage>("ninja status cache")?);
        }
    }
    let ninja_rows = || cached.iter().flat_map(|c| c.vendors.iter().flatten());
    let ninja = Counts {
        record_count: ninja_rows()
            .map(|r| r.npc_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        item_count: cached.len(),
    };
    let coverage = Document::object([
        (super::BACKEND, ninja.document()),
        (crate::foreverdb::BACKEND, foreverdb.document()),
    ]);
    let (mut summary, counts, sources) = match active {
        Some(index) => {
            let counts = super::basis_counts(super::index_bases(index));
            let summary = Document::object([
                ("source", "vendors".into()),
                ("backend", crate::foreverdb::BACKEND.into()),
                ("state", state.name().into()),
                ("cache_items", index.view().item_count.into()),
                ("published_at", index.view().published_at.as_deref().into()),
                ("last_sync_at", index.view().fetched_at.clone().into()),
                ("record_count", index.view().record_count.into()),
            ]);
            (
                summary,
                counts,
                crate::foreverdb::index_sources(index, attempt, context)?,
            )
        }
        None => {
            let counts = super::basis_counts(ninja_rows().map(|r| r.basis));
            let published = cached
                .iter()
                .filter_map(|c| c.published_at.as_deref().filter(|s| !s.is_empty()))
                .max();
            let summary = Document::object([
                ("source", "vendors".into()),
                ("backend", super::BACKEND.into()),
                ("cache_items", cached.len().into()),
                ("published_at", published.into()),
            ]);
            let mut s = source(super::BACKEND, published, context.clock)?;
            s.set(
                "detail",
                crate::human::status::ninja_detail(cached.len(), state),
            )?;
            (summary, counts, vec![s])
        }
    };
    for (key, value) in [
        ("seen", counts[0].into()),
        ("baseline", counts[1].into()),
        ("unverified", counts[2].into()),
        ("foreverdb", info),
        ("coverage", coverage),
    ] {
        summary.set(key, value)?;
    }
    Ok((Payload::decode(summary, "vendor backend status")?, sources))
}
