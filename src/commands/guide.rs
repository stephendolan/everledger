use crate::{
    Context, Result,
    commands::CommandOutput,
    document::Document,
    http::Payload,
    read_roster::{GuideProgress, GuideStep},
};
use serde::Deserialize;

#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(
        required_unless_present = "list",
        conflicts_with = "list",
        help = "Character name or unique first name"
    )]
    pub name: Option<String>,
    #[arg(long, conflicts_with_all = ["search", "limit"], help = "List installed guide families and chapters")]
    pub list: bool,
    #[arg(
        long,
        requires = "list",
        conflicts_with = "name",
        value_name = "GROUP",
        help = "Filter the list by family or subgroup"
    )]
    pub group: Option<String>,
    #[arg(
        long,
        value_name = "TERM",
        help = "Search chapters, step text and targets"
    )]
    pub search: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(i64).range(1..=100), help = "Maximum results (default 5 upcoming or 20 matches)")]
    pub limit: Option<i64>,
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}

#[derive(Deserialize)]
pub struct RouteStep {
    #[serde(flatten)]
    pub step: GuideStep,
    pub steps_from_current: i64,
    pub previous: Option<GuideStep>,
}

#[derive(Deserialize)]
pub struct GuideResponse {
    pub character: String,
    #[serde(flatten)]
    pub progress: GuideProgress,
    pub query: Option<String>,
    pub route_available: Option<bool>,
    pub route_end: Option<String>,
    pub steps: Option<Vec<RouteStep>>,
    pub truncated: Option<bool>,
}

#[derive(Deserialize)]
pub struct CatalogueResponse {
    pub group_query: Option<String>,
    pub groups: Option<Vec<GuideGroup>>,
    pub reason: Option<String>,
    pub sources: Vec<DefinitionSource>,
}

#[derive(Deserialize)]
pub struct GuideGroup {
    pub group: Option<String>,
    pub subgroup: Option<String>,
    pub chapters: Vec<InstalledChapter>,
}

#[derive(Deserialize)]
pub struct InstalledChapter {
    pub chapter: String,
    pub chapter_levels: Option<[i64; 2]>,
    pub step_count: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct DefinitionSource {
    pub file: String,
    pub issues: Vec<String>,
}

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let (data, lines) = if args.list {
        let params = [("group".into(), Document::from(args.group.as_deref()))];
        let response: Payload<CatalogueResponse> =
            crate::roster::roster(context.http, context.config, "/guides", &params)?;
        let lines = crate::human::guide::catalogue(response.view());
        (response.into_document(), lines)
    } else {
        let name = args
            .name
            .as_deref()
            .ok_or_else(|| crate::Failure::usage("guide requires a character name or --list"))?;
        let path = format!("{}/guide", crate::roster::character_path(name));
        let params = [
            ("q".into(), Document::from(args.search.as_deref())),
            ("limit".into(), Document::from(args.limit)),
        ];
        let response: Payload<GuideResponse> =
            crate::roster::roster(context.http, context.config, &path, &params)?;
        let lines = crate::human::guide::route(response.view());
        (response.into_document(), lines)
    };
    let sources = crate::provenance::sources(&data)?;
    let mut output = CommandOutput::new(data, lines);
    output.stale = crate::provenance::is_stale(&sources);
    Ok(output)
}
