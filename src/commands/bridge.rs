use crate::{
    Context, Failure, Result,
    catalogue::{self, Recipe, catalogue_source, load_catalogue},
    commands::CommandOutput,
    document::Document,
    human,
    primitives::casefold,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Color {
    Orange,
    Yellow,
    Green,
    Grey,
    NotLearnable,
}
pub fn color_at(b: &catalogue::Bands, skill: i64) -> Color {
    if skill < b.learn {
        Color::NotLearnable
    } else if b.yellow.is_none_or(|n| skill < n) {
        Color::Orange
    } else if b.green.is_some_and(|n| skill < n) {
        Color::Yellow
    } else if b.grey.is_some_and(|n| skill < n) {
        Color::Green
    } else {
        Color::Grey
    }
}
pub fn color_at_integer(b: &catalogue::Bands, skill: &crate::types::CliInteger) -> Color {
    if skill < &b.learn.into() {
        Color::NotLearnable
    } else if b.yellow.is_none_or(|n| skill < &n.into()) {
        Color::Orange
    } else if b.green.is_some_and(|n| skill < &n.into()) {
        Color::Yellow
    } else if b.grey.is_some_and(|n| skill < &n.into()) {
        Color::Green
    } else {
        Color::Grey
    }
}
pub struct Segment<'a> {
    pub start: crate::types::CliInteger,
    pub end: crate::types::CliInteger,
    pub recipes: Vec<&'a Recipe>,
}
fn priority(r: &Recipe, level: &crate::types::CliInteger) -> (u8, Color, i64) {
    let s = r.source_type.as_deref().unwrap_or("");
    let rank = if s.contains("Trainer") || s.contains("Automatic") {
        0
    } else if s.contains("Vendor") {
        1
    } else if s.contains("Drop") {
        2
    } else {
        3
    };
    (rank, color_at_integer(&r.bands, level), -r.bands.learn)
}
pub fn run(args: &clap::ArgMatches, context: &Context<'_>) -> Result<CommandOutput> {
    let cat = load_catalogue(context)?;
    let prof = casefold(args.get_one::<String>("profession").unwrap());
    let start = args
        .get_one::<crate::types::CliInteger>("start")
        .unwrap()
        .clone();
    let end = args
        .get_one::<crate::types::CliInteger>("end")
        .unwrap()
        .clone();
    let block = cat
        .view
        .professions
        .get(&prof)
        .filter(|_| start < end)
        .ok_or_else(|| Failure::usage("bridge requires a catalogue profession and start < end"))?;
    let rows = block
        .recipes
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            crate::types::CliInteger::from(r.bands.learn) <= end
                && r.bands
                    .grey
                    .is_none_or(|g| crate::types::CliInteger::from(g) > start)
        })
        .collect::<Vec<_>>();
    let mut cuts = vec![start.clone(), end.clone()];
    cuts.extend(rows.iter().filter_map(|(_, r)| {
        if start < r.bands.learn.into() && crate::types::CliInteger::from(r.bands.learn) < end {
            Some(crate::types::CliInteger::from(r.bands.learn))
        } else {
            None
        }
    }));
    cuts.sort();
    cuts.dedup();
    let mut segments = vec![];
    let mut docs = vec![];
    let recipe_docs = cat
        .document
        .get("professions")
        .unwrap()
        .get(&prof)
        .unwrap()
        .get("recipes")
        .unwrap()
        .as_array()
        .unwrap();
    for cut in cuts.windows(2) {
        let (a, b) = (cut[0].clone(), cut[1].clone());
        let mut live = rows
            .iter()
            .filter(|(_, r)| {
                crate::types::CliInteger::from(r.bands.learn) <= a
                    && r.bands
                        .grey
                        .is_none_or(|g| crate::types::CliInteger::from(g) > a)
                    && r.source_type.as_deref() != Some("Not yet known")
            })
            .copied()
            .collect::<Vec<_>>();
        live.sort_by_key(|(_, r)| priority(r, &a));
        live.truncate(12);
        docs.push(Document::object([
            ("from_level", a.document()),
            ("to_level", b.document()),
            (
                "recipes",
                Document::Array(live.iter().map(|(i, _)| recipe_docs[*i].clone()).collect()),
            ),
        ]));
        segments.push(Segment {
            start: a,
            end: b,
            recipes: live.iter().map(|(_, r)| *r).collect(),
        });
    }
    let lines = human::bridge::render(&prof, &start, &end, &segments);
    let data = Document::object([
        ("profession", Document::from(prof)),
        ("segments", Document::Array(docs)),
        (
            "sources",
            Document::Array(vec![catalogue_source(cat, context.clock).document]),
        ),
    ]);
    Ok(CommandOutput::new(data, lines))
}
