#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{
    Context, ExitCode, Result,
    catalogue::{catalogue_source, load_catalogue},
    commands::CommandOutput,
    document::Document,
    human,
};
pub const ANCHORS: [(&str, &str, i64, Option<&str>); 7] = [
    (
        "tailoring",
        "White Swashbuckler's Shirt",
        125,
        Some("Trainer"),
    ),
    ("tailoring", "Bolt of Silk Cloth", 100, Some("Trainer")),
    ("tailoring", "Bolt of Woolen Cloth", 55, Some("Trainer")),
    ("tailoring", "Red Woolen Boots", 75, Some("Pattern · Drop")),
    (
        "tailoring",
        "Dark Silk Shirt",
        120,
        Some("Pattern · Vendor"),
    ),
    ("tailoring", "Robes of Arcana", 125, Some("Pattern · Drop")),
    ("leatherworking", "Cloudy Gustwoven Boots", 55, None),
];
pub enum Check<'a> {
    Count {
        good: bool,
        profession: &'a str,
        actual: usize,
        stated: usize,
    },
    Anchor {
        good: bool,
        name: &'a str,
        recipe: Option<&'a crate::catalogue::Recipe>,
    },
}
impl Check<'_> {
    pub fn good(&self) -> bool {
        match self {
            Self::Count { good, .. } | Self::Anchor { good, .. } => *good,
        }
    }
}
pub fn run(_args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    context
        .diagnostics
        .warn("deprecated verify; use: everledger status")?;
    let cat = load_catalogue(context)?;
    let (ok, lines) = check_catalogue(cat);
    let data = Document::object([
        ("ok", Document::from(ok)),
        (
            "checks",
            Document::Array(lines.iter().cloned().map(Document::from).collect()),
        ),
        (
            "sources",
            Document::Array(vec![catalogue_source(cat, context.clock)?.into_document()]),
        ),
    ]);
    let mut out = CommandOutput::new(data, lines);
    out.code = if ok {
        ExitCode::Answer
    } else {
        ExitCode::Error
    };
    Ok(out)
}

pub fn check_catalogue(cat: &crate::catalogue::Catalogue) -> (bool, Vec<String>) {
    let mut checks = vec![];
    for (prof, block) in &cat.view().professions.0 {
        checks.push(Check::Count {
            good: block.recipes.len() == block.stated_count,
            profession: prof,
            actual: block.recipes.len(),
            stated: block.stated_count,
        });
    }
    for (prof, name, learn, src) in ANCHORS {
        let recipe = cat
            .view()
            .professions
            .get(prof)
            .and_then(|p| p.recipes.iter().find(|r| r.name == name));
        let good = recipe.is_some_and(|r| {
            r.bands.learn == learn && (src.is_none() || r.source_type.as_deref() == src)
        });
        checks.push(Check::Anchor { good, name, recipe });
    }
    let ok = checks.iter().all(Check::good);
    let lines = human::verify::render(&checks, cat);
    (ok, lines)
}
