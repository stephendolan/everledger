use crate::{
    catalogue::Recipe,
    commands::bridge::color_at,
    human::{bridge::color, integer},
};
pub fn recipe(prof: &str, r: &Recipe, skill: Option<i64>) -> String {
    let b = &r.bands;
    let bands = match b.yellow {
        Some(y) => format!(
            "learn {}, yellow {y}, green {}, grey {}",
            b.learn,
            integer(b.green),
            integer(b.grey)
        ),
        None => format!("learn {} (stays orange)", b.learn),
    };
    let col = skill
        .map(|s| format!(" | at skill {s}: {}", color(color_at(b, s))))
        .unwrap_or_default();
    let reags = if r.reagents.is_empty() {
        "none".into()
    } else {
        r.reagents
            .iter()
            .map(|x| format!("{}x {}", x.qty, x.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let res = &r.result;
    let item = res
        .item_id
        .filter(|n| *n != 0)
        .map(|n| format!(" (item {n})"))
        .unwrap_or_default();
    let qty = if res.qty > 1 {
        format!(" x{}", res.qty)
    } else {
        String::new()
    };
    format!(
        "{} [{prof}] spell {}\n  bands: {bands}{col}\n  makes: {}{item}{qty}\n  reagents: {reags}\n  source: {}",
        r.name,
        r.spell_id,
        res.name,
        r.source_type
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("unmarked in index (see recipe page)")
    )
}
pub fn knowledge(known: &[Option<&str>], unavailable: &[Option<&str>]) -> crate::Result<String> {
    let join = |names: &[Option<&str>]| {
        names
            .iter()
            .map(|n| n.ok_or_else(|| crate::Failure::shape("roster character", "missing name")))
            .collect::<crate::Result<Vec<_>>>()
            .map(|n| n.join(", "))
    };
    Ok(format!(
        "  known by: {}{}",
        if known.is_empty() {
            "no roster character".into()
        } else {
            join(known)?
        },
        if unavailable.is_empty() {
            String::new()
        } else {
            format!("; recipe data unavailable: {}", join(unavailable)?)
        }
    ))
}
pub fn unchecked(e: &crate::Failure) -> String {
    format!("known by: not checked ({})", e.message)
}
