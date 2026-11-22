use crate::{Context, Result, http::Payload, provenance::Field};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize)]
pub struct KnownRecipe {
    pub spell_id: i64,
}
#[derive(Clone, Debug, Deserialize, Default)]
pub struct RecipeData {
    pub available: Option<bool>,
    pub known: Option<Vec<KnownRecipe>>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Profession {
    pub name: String,
    pub skill_available: Option<bool>,
    #[serde(default)]
    pub skill: Field<i64>,
    #[serde(default)]
    pub max_skill: Field<i64>,
    pub reason: Option<String>,
    pub recipes: Option<RecipeData>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Hearth {
    pub location: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct GuideStep {
    pub chapter: String,
    pub step: i64,
    pub text: Vec<String>,
    pub targets: Vec<String>,
    pub level_tags: Vec<String>,
    pub chapter_levels: Option<[i64; 2]>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct GuideProgress {
    pub available: bool,
    pub guide_group: Option<String>,
    pub guide_subgroup: Option<String>,
    pub chapter: Option<String>,
    pub current_step: Option<i64>,
    pub current: Option<GuideStep>,
    pub current_reason: Option<String>,
    pub reason: Option<String>,
    pub saved_at: Option<String>,
    pub file_age_seconds: Option<i64>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct TalentSnapshot {
    pub available: bool,
    pub captured_at: Option<String>,
    pub reason: Option<String>,
    #[serde(default)]
    pub trees: Vec<TalentTree>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct TalentTree {
    pub name: String,
    pub points: u64,
    pub talents: Vec<RankedTalent>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct RankedTalent {
    pub name: String,
    pub rank: u64,
    pub max_rank: u64,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Character {
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub level: Option<crate::primitives::Scalar>,
    pub race: Option<String>,
    #[serde(rename = "class")]
    pub class_name: Option<String>,
    pub faction: Option<String>,
    pub realm: Option<String>,
    pub gold_copper: Option<crate::Copper>,
    pub hearth: Option<Hearth>,
    pub guide: Option<GuideProgress>,
    pub talents: Option<TalentSnapshot>,
    pub professions_updated_at: Option<String>,
    #[serde(default)]
    pub professions: Vec<Profession>,
}
#[derive(Debug, Deserialize)]
pub struct Characters {
    pub characters: Vec<Character>,
}
#[derive(Debug, Deserialize)]
pub struct CharacterResponse {
    pub character: Character,
    pub generated_at: Option<String>,
}
pub fn character(context: &Context<'_>, name: &str) -> Result<Payload<CharacterResponse>> {
    crate::roster::roster(
        context.http,
        context.config,
        &crate::roster::character_path(name),
        &[],
    )
}
#[derive(Debug, PartialEq, Eq)]
pub enum Knowledge {
    Unavailable,
    Available(BTreeSet<i64>),
}
pub fn known_ids(ch: &Character, profession: &str) -> Knowledge {
    let p = ch
        .professions
        .iter()
        .find(|p| crate::primitives::lower(&p.name) == crate::primitives::lower(profession));
    match p.and_then(|p| p.recipes.as_ref()) {
        Some(RecipeData {
            available: Some(true),
            known: Some(ids),
        }) => Knowledge::Available(ids.iter().map(|r| r.spell_id).collect()),
        _ => Knowledge::Unavailable,
    }
}
