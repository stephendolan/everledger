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
    #[arg(help = "Character name or unique first name")]
    pub name: String,
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

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let path = format!("{}/guide", crate::roster::character_path(&args.name));
    let params = [
        ("q".into(), Document::from(args.search.as_deref())),
        ("limit".into(), Document::from(args.limit)),
    ];
    let response: Payload<GuideResponse> =
        crate::roster::roster(context.http, context.config, &path, &params)?;
    let lines = crate::human::guide::route(response.view());
    let sources = crate::provenance::sources(response.document())?;
    let mut output = CommandOutput::new(response.into_document(), lines);
    output.stale = crate::provenance::is_stale(&sources);
    Ok(output)
}
