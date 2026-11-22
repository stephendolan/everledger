use crate::{
    Result,
    catalogue::{self, Catalogue},
    commands::bridge::color_at,
    human::{bridge::color, copper_money, integer, text, value},
    read_roster::{Character, CharacterResponse, Knowledge, known_ids},
};
fn summary(c: &Character) -> String {
    format!(
        "{} | level {} {} {}",
        text(c.name.as_deref()),
        value(c.level.as_ref()),
        text(c.race.as_deref()),
        text(c.class_name.as_deref())
    )
}
pub fn list(chars: &[Character]) -> Vec<String> {
    if chars.is_empty() {
        vec!["no characters recorded".into()]
    } else {
        chars
            .iter()
            .map(|c| format!("{} | gold {}", summary(c), copper_money(c.gold_copper)))
            .collect()
    }
}
pub fn character(
    s: &crate::http::Payload<CharacterResponse>,
    cat: Option<&Catalogue>,
    profession: Option<&str>,
    skill: Option<i64>,
) -> Result<Vec<String>> {
    let c = &s.view().character;
    let mut lines = vec![
        format!(
            "{} | {} {} | gold {} | hearth {}",
            summary(c),
            text(c.faction.as_deref()),
            text(c.realm.as_deref()),
            copper_money(c.gold_copper),
            text(c.hearth.as_ref().and_then(|h| h.location.as_deref()))
        ),
        format!(
            "  (roster snapshot generated {}; professions updated {})",
            text(s.view().generated_at.as_deref()),
            text(c.professions_updated_at.as_deref())
        ),
    ];
    if let Some(guide) = &c.guide {
        lines.push(crate::human::guide::progress(guide));
    }
    let want = profession.map(crate::primitives::lower);
    for p in &c.professions {
        if p.skill_available == Some(true) {
            if matches!(p.max_skill, crate::provenance::Field::Missing)
                || matches!(p.skill, crate::provenance::Field::Missing)
            {
                return Err(crate::Failure::shape(
                    "roster profession",
                    "missing skill or max_skill",
                ));
            }
            lines.push(format!(
                "  {}: {}/{}",
                p.name,
                integer(p.skill.as_ref().copied()),
                integer(p.max_skill.as_ref().copied())
            ));
        } else {
            lines.push(format!("  {}: skill unavailable from roster ({}); open the profession window in game, or pass --skill",p.name,text(p.reason.as_deref())));
        }
        let knowledge = known_ids(c, &p.name);
        lines.push(match &knowledge {
            Knowledge::Unavailable => {
                "    known recipes: unavailable from roster (never read as 'knows nothing')".into()
            }
            Knowledge::Available(ids) => format!(
                "    known recipes: {} (roster, DataStore_Crafts)",
                p.recipes
                    .as_ref()
                    .and_then(|r| r.known.as_ref())
                    .map_or(ids.len(), Vec::len)
            ),
        });
        if want.as_deref() != Some(crate::primitives::lower(&p.name).as_str()) {
            continue;
        }
        let Some(sk) = skill.or(p.skill.as_ref().copied()) else {
            lines.push("    no skill to analyse; pass --skill N".into());
            continue;
        };
        let Some((cat, want, block)) = cat
            .zip(want.as_deref())
            .and_then(|(c, w)| c.view().professions.get(w).map(|b| (c, w, b)))
        else {
            lines.push(format!(
                "    no catalogue profession '{}'",
                want.as_deref().unwrap_or("")
            ));
            continue;
        };
        let tag = |id| match &knowledge {
            Knowledge::Unavailable => "",
            Knowledge::Available(ids) => {
                if ids.contains(&id) {
                    "known "
                } else {
                    "TRAIN "
                }
            }
        };
        let mut ups = block
            .recipes
            .iter()
            .filter(|r| r.bands.learn <= sk && r.bands.grey.is_none_or(|g| g > sk))
            .collect::<Vec<_>>();
        ups.sort_by_key(|r| (color_at(&r.bands, sk), std::cmp::Reverse(r.bands.learn)));
        lines.push(format!(
            "    skill-up options at {sk}:{}",
            match &knowledge {
                Knowledge::Unavailable => "",
                Knowledge::Available(_) => " (TRAIN = not known; training/pattern cost applies)",
            }
        ));
        for r in ups.into_iter().take(8) {
            lines.push(format!(
                "      {}{:<34} {:<6} learn {:<4} grey {}  {}",
                tag(r.spell_id),
                r.name,
                color(color_at(&r.bands, sk)),
                r.bands.learn,
                integer(r.bands.grey),
                r.source_type
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("?")
            ));
        }
        let end = sk
            .checked_add(30)
            .ok_or_else(|| crate::Failure::error("skill out of range"))?;
        let mut next = block
            .recipes
            .iter()
            .filter(|r| sk < r.bands.learn && r.bands.learn <= end)
            .collect::<Vec<_>>();
        next.sort_by_key(|r| r.bands.learn);
        lines.push(format!("    next unlocks (to {end}):"));
        for r in next.into_iter().take(8) {
            lines.push(format!(
                "      {}{:<34} learn {}  {}",
                tag(r.spell_id),
                r.name,
                r.bands.learn,
                r.source_type
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("?")
            ));
        }
        if let Knowledge::Available(ids) = &knowledge {
            let mut index = std::collections::BTreeMap::new();
            for r in catalogue::recipes(cat)? {
                index.insert(r.recipe.spell_id, (r.profession, &r.recipe.name));
            }
            let mut names = ids
                .iter()
                .filter_map(|id| index.get(id))
                .filter(|(p, _)| *p == want)
                .map(|(_, n)| n.as_str())
                .collect::<Vec<_>>();
            names.sort();
            let extra = ids.len() - names.len();
            lines.push(format!("    all known {} recipes ({}):", p.name, ids.len()));
            for chunk in names.chunks(4) {
                lines.push(format!("      {}", chunk.join(", ")));
            }
            if extra != 0 {
                lines.push(format!(
                    "      (+{extra} known spell ids not in the catalogue for this profession)"
                ));
            }
        }
    }
    Ok(lines)
}
