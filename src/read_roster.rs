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
    #[serde(default)]
    pub character: Field<Character>,
    pub generated_at: Option<String>,
}
pub struct CharacterSnapshot {
    pub payload: Payload<CharacterResponse>,
    pub character: Character,
}
pub fn character(context: &Context<'_>, name: &str) -> Result<CharacterSnapshot> {
    let payload: Payload<CharacterResponse> = crate::roster::roster(
        context.http,
        context.config,
        &crate::roster::character_path(name),
        &[],
    )?;
    let character = match &payload.view().character {
        Field::Missing => payload.document().decode("roster character")?,
        Field::Null => {
            return Err(crate::Failure::shape(
                "roster character",
                "character is null",
            ));
        }
        Field::Value(ch) => ch.clone(),
    };
    Ok(CharacterSnapshot { payload, character })
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
