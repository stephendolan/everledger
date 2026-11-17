use crate::{
    Context, Failure, Result,
    catalogue::{Item, nullable},
    document::Document,
    foreverdb::{Index, Seller},
    http::Payload,
};
use serde::Deserialize;
use std::collections::BTreeSet;
#[derive(Clone, Debug, Deserialize)]
pub struct Snapshot {
    pub item: Item,
    #[serde(deserialize_with = "nullable")]
    pub vendors: Option<Vec<Seller>>,
    #[serde(deserialize_with = "nullable")]
    pub reason: Option<String>,
    pub known_zones: Option<Vec<String>>,
    pub fallback_additional_sellers: usize,
    #[serde(default, rename = "_fallback_vendors")]
    fallback: Vec<super::Seller>,
}
#[derive(Clone, Debug)]
pub struct ForeverdbAnswer {
    payload: Payload<Snapshot>,
}
impl ForeverdbAnswer {
    pub fn payload(&self) -> &Payload<Snapshot> {
        &self.payload
    }
    pub fn rows(&self) -> &[Seller] {
        self.payload.view().vendors.as_deref().unwrap_or(&[])
    }
    pub(super) fn lookup(item_id: i64, index: &Index, context: &Context<'_>) -> Result<Self> {
        let mut data = crate::foreverdb::lookup(item_id, index, context)?;
        let path = context
            .config
            .data_dir
            .join("vendors")
            .join(super::BACKEND)
            .join(format!("{item_id}.json"));
        let fallback = match std::fs::read_to_string(path) {
            Ok(text) => {
                let cached = crate::document::parse(&text)?;
                if super::cache_matches(&cached, context)? {
                    let view: super::CachedCoverage = cached.decode("ninja comparison cache")?;
                    if view.vendors.is_some() {
                        cached
                            .get("vendors")
                            .cloned()
                            .ok_or_else(|| Failure::shape("ninja cache", "missing vendors"))?
                    } else {
                        Document::Array(vec![])
                    }
                } else {
                    Document::Array(vec![])
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Document::Array(vec![]),
            Err(e) => return Err(e.into()),
        };
        data.set("_fallback_vendors", fallback)?;
        data.set("fallback_additional_sellers", 0usize)?;
        let mut answer = Self {
            payload: Payload::decode(data, "ForeverDB answer")?,
        };
        answer.compare_coverage()?;
        Ok(answer)
    }
    fn compare_coverage(&mut self) -> Result<()> {
        let known = self
            .rows()
            .iter()
            .map(|r| r.npc_id)
            .collect::<BTreeSet<_>>();
        let fallback = self
            .payload
            .view()
            .fallback
            .iter()
            .map(|r| r.npc_id)
            .collect::<BTreeSet<_>>();
        self.payload.set(
            "fallback_additional_sellers",
            fallback.difference(&known).count(),
        )
    }
    pub fn select_zone(&mut self, query: &str) -> Result<()> {
        let zones = self
            .rows()
            .iter()
            .map(|r| r.zone.clone())
            .collect::<BTreeSet<_>>();
        let q = crate::primitives::casefold(query);
        let q = super::zone_query(&q);
        if let Some(rows) = &self.payload.view().vendors {
            let docs = self
                .payload
                .document()
                .get("vendors")
                .and_then(Document::as_array)
                .ok_or_else(|| Failure::shape("ForeverDB answer", "missing vendors"))?;
            let filtered = rows
                .iter()
                .zip(docs)
                .filter(|(r, _)| matches_zone(&r.zone, &r.zone_slug, q))
                .map(|(_, d)| d.clone())
                .collect();
            self.payload.set("vendors", Document::Array(filtered))?;
        }
        let docs = self
            .payload
            .document()
            .get("_fallback_vendors")
            .and_then(Document::as_array)
            .ok_or_else(|| Failure::shape("ForeverDB answer", "missing comparison vendors"))?;
        let filtered = self
            .payload
            .view()
            .fallback
            .iter()
            .zip(docs)
            .filter(|(r, _)| matches_zone(&r.zone, &r.zone_slug, q))
            .map(|(_, d)| d.clone())
            .collect();
        self.payload
            .set("_fallback_vendors", Document::Array(filtered))?;
        self.compare_coverage()?;
        self.payload
            .set("known_zones", Document::from_typed(&zones)?)?;
        Ok(())
    }
    pub fn public(&self) -> Document {
        let mut data = self.payload.document().clone();
        data.remove("_fallback_vendors");
        data
    }
    pub fn sellers_document(&self) -> Result<Document> {
        self.payload
            .document()
            .get("vendors")
            .cloned()
            .ok_or_else(|| Failure::shape("ForeverDB answer", "missing vendors"))
    }
    pub fn reason(&self) -> Option<&str> {
        self.payload.view().reason.as_deref()
    }
    pub fn sources(&self) -> Result<Vec<crate::provenance::Source>> {
        crate::provenance::sources(self.payload.document())
    }
}
fn matches_zone(zone: &str, slug: &str, q: &str) -> bool {
    crate::primitives::casefold(zone).contains(q) || crate::primitives::casefold(slug).contains(q)
}
