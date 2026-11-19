use crate::{
    Context, Failure, Result,
    catalogue::{self, Recipe, catalogue_source, load_catalogue},
    commands::CommandOutput,
    document::Document,
    human,
    primitives::casefold,
};

#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers = true)]
    pub profession: String,
    #[arg(value_parser=crate::cli::parse_integer,allow_negative_numbers=true)]
    pub start: i64,
    #[arg(value_parser=crate::cli::parse_integer,allow_negative_numbers=true)]
    pub end: i64,
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}
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
pub struct Segment<'a> {
    pub start: i64,
    pub end: i64,
    pub recipes: Vec<&'a Recipe>,
}
fn priority(r: &Recipe, level: i64) -> (u8, Color, std::cmp::Reverse<i64>) {
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
    (
        rank,
        color_at(&r.bands, level),
        std::cmp::Reverse(r.bands.learn),
    )
}
pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let cat = load_catalogue(context)?;
    let prof = casefold(&args.profession);
    let (start, end) = (args.start, args.end);
    if cat.view().professions.get(&prof).is_none() || start >= end {
        return Err(Failure::usage(
            "bridge requires a catalogue profession and start < end",
        ));
    }
    let rows = catalogue::recipes(cat)?
        .into_iter()
        .filter(|r| {
            r.profession == prof
                && r.recipe.bands.learn <= end
                && r.recipe.bands.grey.is_none_or(|g| g > start)
        })
        .collect::<Vec<_>>();
    let mut cuts = vec![start, end];
    cuts.extend(rows.iter().filter_map(|r| {
        let learn = r.recipe.bands.learn;
        (start < learn && learn < end).then_some(learn)
    }));
    cuts.sort();
    cuts.dedup();
    let mut segments = vec![];
    let mut docs = vec![];
    for cut in cuts.windows(2) {
        let (a, b) = (cut[0], cut[1]);
        let mut live = rows
            .iter()
            .filter(|r| {
                r.recipe.bands.learn <= a
                    && r.recipe.bands.grey.is_none_or(|g| g > a)
                    && r.recipe.source_type.as_deref() != Some("Not yet known")
            })
            .collect::<Vec<_>>();
        live.sort_by_key(|r| priority(r.recipe, a));
        live.truncate(12);
        docs.push(Document::object([
            ("from_level", Document::from(a)),
            ("to_level", Document::from(b)),
            (
                "recipes",
                Document::Array(live.iter().map(|r| r.document.clone()).collect()),
            ),
        ]));
        segments.push(Segment {
            start: a,
            end: b,
            recipes: live.iter().map(|r| r.recipe).collect(),
        });
    }
    let lines = human::bridge::render(&prof, start, end, &segments);
    let data = Document::object([
        ("profession", Document::from(prof)),
        ("segments", Document::Array(docs)),
        (
            "sources",
            Document::Array(vec![catalogue_source(cat, context.clock)?.into_document()]),
        ),
    ]);
    Ok(CommandOutput::new(data, lines))
}
