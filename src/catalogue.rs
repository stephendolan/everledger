use crate::{
    Context, Failure, Result,
    clock::Clock,
    config::Config,
    document::Document,
    http::Payload,
    primitives::casefold,
    provenance::{Source, source},
};
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use std::{fmt, marker::PhantomData, sync::OnceLock};

pub const ADVISORY_DAYS: i64 = 14;

#[derive(Clone, Debug)]
pub struct OrderedMap<T>(pub Vec<(String, T)>);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for OrderedMap<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = OrderedMap<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut entries: Vec<(String, T)> = vec![];
                while let Some((k, v)) = map.next_entry::<String, T>()? {
                    if let Some((_, old)) = entries.iter_mut().find(|(key, _)| *key == k) {
                        *old = v;
                    } else {
                        entries.push((k, v));
                    }
                }
                Ok(OrderedMap(entries))
            }
        }
        d.deserialize_map(V(PhantomData))
    }
}
impl<T> OrderedMap<T> {
    pub fn get(&self, key: &str) -> Option<&T> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}
pub fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(d)
}
#[derive(Clone, Debug, Deserialize)]
pub struct Bands {
    pub learn: i64,
    #[serde(deserialize_with = "nullable")]
    pub yellow: Option<i64>,
    #[serde(deserialize_with = "nullable")]
    pub green: Option<i64>,
    #[serde(deserialize_with = "nullable")]
    pub grey: Option<i64>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct RecipeItem {
    pub item_id: Option<i64>,
    pub name: String,
    pub qty: i64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Recipe {
    pub spell_id: i64,
    pub name: String,
    pub bands: Bands,
    pub result: RecipeItem,
    pub reagents: Vec<RecipeItem>,
    #[serde(deserialize_with = "nullable")]
    pub source_type: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Profession {
    pub stated_count: usize,
    pub recipes: Vec<Recipe>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct CatalogueData {
    pub generated_at: String,
    #[serde(default)]
    pub source: crate::provenance::Field<String>,
    pub professions: OrderedMap<Profession>,
}
pub type Catalogue = Payload<CatalogueData>;
#[derive(Default)]
pub struct CatalogueStore {
    cached: OnceLock<Catalogue>,
}
impl CatalogueStore {
    pub fn load(
        &self,
        config: &Config,
        clock: &dyn Clock,
        diagnostics: &dyn crate::diagnostics::Diagnostics,
    ) -> Result<&Catalogue> {
        if self.cached.get().is_none() {
            let cat = load_catalogue_file(config, clock, diagnostics)?;
            let _ = self.cached.set(cat);
        }
        self.cached
            .get()
            .ok_or_else(|| Failure::error("catalogue initialization failed"))
    }
    pub fn from_document(document: Document) -> Result<Self> {
        let catalogue = Payload::decode(document, "catalogue")?;
        let store = Self::default();
        let _ = store.cached.set(catalogue);
        Ok(store)
    }
}
pub fn load_catalogue<'a>(context: &'a Context<'_>) -> Result<&'a Catalogue> {
    context
        .catalogue
        .load(context.config, context.clock, context.diagnostics)
}
fn load_catalogue_file(
    config: &Config,
    clock: &dyn Clock,
    diagnostics: &dyn crate::diagnostics::Diagnostics,
) -> Result<Catalogue> {
    let text = std::fs::read_to_string(config.data_dir.join("catalogue.json")).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Failure::error("catalogue missing; run: everledger refresh catalogue")
        } else {
            e.into()
        }
    })?;
    let cat: Catalogue = Payload::decode(crate::document::parse(&text)?, "catalogue")?;
    if crate::time::age(Some(&cat.view().generated_at), clock)
        .is_some_and(|s| s > ADVISORY_DAYS * 86400)
    {
        diagnostics.warn(&format!(
            "catalogue is older than {ADVISORY_DAYS}d; run: everledger refresh catalogue"
        ))?;
    }
    Ok(cat)
}
pub fn catalogue_source(cat: &Catalogue, clock: &dyn Clock) -> Result<Source> {
    use crate::provenance::Field;
    let name = match &cat.view().source {
        Field::Missing => "catalogue",
        Field::Null => "",
        Field::Value(s) => s,
    };
    let mut s = source(name, Some(&cat.view().generated_at), clock)?;
    if matches!(cat.view().source, Field::Null) {
        s.set("name", Document::Null)?;
    }
    Ok(s)
}
pub struct RecipeRef<'a> {
    pub profession: &'a str,
    pub recipe: &'a Recipe,
    pub document: &'a Document,
}
pub fn recipes(cat: &Catalogue) -> Result<Vec<RecipeRef<'_>>> {
    let mut result = vec![];
    let professions = cat
        .document()
        .get("professions")
        .ok_or_else(|| Failure::shape("catalogue", "missing professions"))?;
    for (prof, block) in &cat.view().professions.0 {
        let docs = professions
            .get(prof)
            .and_then(|p| p.get("recipes"))
            .and_then(Document::as_array)
            .ok_or_else(|| Failure::shape("catalogue", "missing recipes array"))?;
        if docs.len() != block.recipes.len() {
            return Err(Failure::shape("catalogue", "recipe view mismatch"));
        }
        for (recipe, document) in block.recipes.iter().zip(docs) {
            result.push(RecipeRef {
                profession: prof,
                recipe,
                document,
            });
        }
    }
    Ok(result)
}
#[derive(Clone, Debug, Deserialize, serde::Serialize)]
pub struct Item {
    pub id: i64,
    pub name: Option<String>,
}
fn recipe_items(cat: &Catalogue) -> Result<impl Iterator<Item = &RecipeItem>> {
    Ok(recipes(cat)?
        .into_iter()
        .flat_map(|r| std::iter::once(&r.recipe.result).chain(&r.recipe.reagents)))
}
fn resolve_name(query: &str, cat: &Catalogue) -> Result<Item> {
    let mut hits = vec![];
    let query = casefold(query);
    for item in recipe_items(cat)? {
        if let Some(id) = item.item_id.filter(|id| *id != 0)
            && casefold(&item.name) == query
        {
            if let Some((_, name)) = hits.iter_mut().find(|(i, _)| *i == id) {
                *name = item.name.clone();
            } else {
                hits.push((id, item.name.clone()));
            }
        }
    }
    if hits.len() > 1 {
        return Err(Failure::usage(format!(
            "ambiguous item: {}",
            hits.iter()
                .map(|(id, name)| format!("{name} ({id})"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    hits.into_iter()
        .next()
        .map(|(id, name)| Item {
            id,
            name: Some(name),
        })
        .ok_or_else(|| Failure::error("item not found in catalogue; use a numeric item ID"))
}
pub fn item_for(query: &str, context: &Context<'_>) -> Result<Item> {
    if !query.is_empty() && query.bytes().all(|b| b.is_ascii_digit()) {
        let id = query
            .parse::<i64>()
            .ok()
            .ok_or_else(|| Failure::error("invalid integer item ID"))?;
        let name = if let Ok(cat) = load_catalogue(context) {
            recipe_items(cat)?
                .find(|r| r.item_id == Some(id))
                .map(|r| r.name.clone())
        } else {
            None
        };
        Ok(Item { id, name })
    } else {
        resolve_name(query, load_catalogue(context)?)
    }
}
#[derive(Clone, Debug, Deserialize, Default)]
pub struct RecordedItem {
    #[serde(default)]
    pub id: crate::provenance::Field<i64>,
    #[serde(default)]
    pub name: crate::provenance::Field<String>,
    pub id_basis: Option<String>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ItemRef<'a> {
    pub id: Option<i64>,
    pub name: Option<&'a str>,
    pub id_basis: Option<&'a str>,
}
impl RecordedItem {
    pub fn reference(&self) -> ItemRef<'_> {
        ItemRef {
            id: self.id.as_ref().copied(),
            name: self.name.as_ref().map(String::as_str),
            id_basis: self.id_basis.as_deref(),
        }
    }
}
impl ItemRef<'_> {
    pub fn matched_by_name(self) -> bool {
        self.id_basis == Some("name")
            || self.id.is_none() && self.name.is_some_and(|s| !s.is_empty())
    }
}
pub fn item_matches(row: ItemRef<'_>, item: &Item) -> bool {
    if let Some(id) = row.id {
        return id == item.id;
    }
    match (row.name, item.name.as_deref()) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => casefold(a) == casefold(b),
        _ => false,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchBasis {
    Id,
    Name,
}
pub fn item_basis(row: ItemRef<'_>) -> MatchBasis {
    if row.id.is_some() && row.id_basis != Some("name") {
        MatchBasis::Id
    } else {
        MatchBasis::Name
    }
}
pub fn disclose_item_source(
    data: &mut Document,
    item: Option<&Item>,
    context: &Context<'_>,
) -> Result<()> {
    if item.is_some_and(|i| i.name.as_ref().is_some_and(|n| !n.is_empty())) {
        let s = catalogue_source(load_catalogue(context)?, context.clock)?;
        let mut used = crate::provenance::sources(data)?;
        if !used.iter().any(|a| a.document() == s.document()) {
            used.push(s);
        }
        data.set(
            "sources",
            Document::Array(used.into_iter().map(|s| s.into_document()).collect()),
        )?;
    }
    Ok(())
}
