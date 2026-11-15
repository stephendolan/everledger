use crate::{
    Result,
    clock::Clock,
    document::{Document, JsonMode, dumps},
    primitives::human,
    provenance::{Field, Source, SourceAge, sources},
};
use std::collections::BTreeSet;
pub mod bridge;
pub mod verify;
pub use crate::primitives::{copper_money, money};

pub type SourceFamilies = Vec<(String, Vec<Source>)>;
struct Group {
    name: String,
    times: BTreeSet<String>,
    ages: Vec<Option<SourceAge>>,
    credits: BTreeSet<String>,
    files: BTreeSet<String>,
    family: bool,
}
fn nonempty(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}
pub fn source_lines(items: &[Source], families: &SourceFamilies, clock: &dyn Clock) -> String {
    let mut groups: Vec<Group> = vec![];
    for s in items {
        let v = &s.view;
        let fallback = nonempty(v.name.as_deref())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                std::path::Path::new(
                    nonempty(v.file.as_deref())
                        .or(nonempty(v.path.as_deref()))
                        .unwrap_or("roster"),
                )
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
            });
        let family = families
            .iter()
            .find(|(_, members)| members.iter().any(|m| m.document == s.document))
            .map(|(label, _)| label);
        let name = family.cloned().unwrap_or(fallback);
        let when = nonempty(v.saved_at.as_deref())
            .or(nonempty(v.scanned_at.as_deref()))
            .or(nonempty(v.mtime.as_deref()))
            .or(nonempty(v.updated_at.as_deref()));
        let seconds = match &v.scan_age_seconds {
            Field::Missing => match &v.file_age_seconds {
                Field::Missing => match &v.age_seconds {
                    Field::Missing => crate::time::age(when, clock).map(SourceAge::Integer),
                    Field::Null => None,
                    Field::Value(a) => Some(*a),
                },
                Field::Null => None,
                Field::Value(a) => Some(*a),
            },
            Field::Null => None,
            Field::Value(a) => Some(*a),
        };
        let index = groups
            .iter()
            .position(|g| g.name == name)
            .unwrap_or_else(|| {
                groups.push(Group {
                    name: name.clone(),
                    times: BTreeSet::new(),
                    ages: vec![],
                    credits: BTreeSet::new(),
                    files: BTreeSet::new(),
                    family: family.is_some(),
                });
                groups.len() - 1
            });
        let group = &mut groups[index];
        group.times.insert(human(&Document::from(when)));
        if !group.ages.contains(&seconds) {
            group.ages.push(seconds);
        }
        group.files.insert(
            nonempty(v.file.as_deref())
                .or(nonempty(v.path.as_deref()))
                .or(nonempty(v.name.as_deref()))
                .unwrap_or("unknown")
                .into(),
        );
        if let Some(detail) = &v.detail
            && detail.starts_with("Data from ")
        {
            group
                .credits
                .insert(detail.split(';').next().unwrap().into());
        }
    }
    let summaries = groups
        .into_iter()
        .map(|g| {
            let mut ages = g.ages.iter().flatten().copied().collect::<Vec<_>>();
            ages.sort_by(|a, b| a.seconds().total_cmp(&b.seconds()));
            let text = SourceAge::text;
            let unknown = if !ages.is_empty() && g.ages.contains(&None) {
                ", some unknown"
            } else {
                ""
            };
            if g.family {
                let fresh = if ages.is_empty() {
                    "age unknown".into()
                } else {
                    format!(
                        "oldest age {}s, newest {}s{unknown}",
                        text(*ages.last().unwrap()),
                        text(ages[0])
                    )
                };
                format!("{} ({} files; {fresh})", g.name, g.files.len())
            } else {
                let fresh = if ages.is_empty() {
                    "unknown".into()
                } else {
                    let span = if ages.len() > 1 {
                        format!("{}-{}", text(ages[0]), text(*ages.last().unwrap()))
                    } else {
                        text(ages[0])
                    };
                    format!("{span}s{unknown}")
                };
                let times = g.times.into_iter().collect::<Vec<_>>();
                let saved = if times.len() == 1 {
                    times[0].clone()
                } else {
                    format!("{}..{}", times[0], times.last().unwrap())
                };
                let credit = if g.credits.is_empty() {
                    String::new()
                } else {
                    format!("; {}", g.credits.into_iter().collect::<Vec<_>>().join("; "))
                };
                format!("{} (saved/data {saved}; age {fresh}){credit}", g.name)
            }
        })
        .collect::<Vec<_>>();
    format!(
        "source: {}",
        if summaries.is_empty() {
            "unknown (age unknown)".into()
        } else {
            summaries.join("; ")
        }
    )
}
pub fn emit(
    data: &Document,
    lines: &[String],
    as_json: bool,
    stale: Option<bool>,
    families: &SourceFamilies,
    clock: &dyn Clock,
) -> Result<String> {
    if as_json {
        return Ok(dumps(data, JsonMode::Stdout) + "\n");
    }
    let items = sources(data)?;
    let stale = stale.unwrap_or_else(|| {
        items.iter().any(|s| {
            s.view.stale == Some(true)
                && !s
                    .view
                    .file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or(s.view.name.as_deref())
                    .unwrap_or("")
                    .contains("Journalator")
        })
    });
    let mut lines = lines.to_vec();
    if stale {
        lines.push("stale: data exceeds its freshness limit".into());
    }
    lines.push(source_lines(&items, families, clock));
    Ok(lines.join("\n") + "\n")
}
pub mod char;
pub mod events;
pub mod expirations;
pub mod listings;
pub mod margins;
pub mod net;
pub mod positions;
pub mod price;
pub mod purchases;
pub mod recipe;
pub mod refresh;
pub mod sales;
pub mod status;
pub mod vendors;
