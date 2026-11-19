#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers=true,required=true,num_args=1..)]
    pub item: Vec<String>,
    #[arg(
        long = "in",
        allow_negative_numbers = true,
        help = "zone/city substring; aliases IF, SW, Darn, Org, TB, UC"
    )]
    pub zone: Option<String>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{Context, Result, commands::CommandOutput, vendors};
pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let item = crate::catalogue::item_for(&args.item.join(" "), context)?;
    let mut answer = vendors::lookup(item.id, context)?;
    if let Some(q) = args.zone.as_deref().filter(|s| !s.is_empty()) {
        answer.select_zone(q)?;
    }
    let mut data = answer.public();
    crate::catalogue::disclose_item_source(&mut data, Some(&item), context)?;
    let lines = crate::human::vendors::render(&answer);
    let stale = crate::provenance::is_stale(&crate::provenance::sources(&data)?);
    let mut out = CommandOutput::new(data, lines);
    out.stale = stale;
    if stale {
        out.code = crate::ExitCode::Stale;
    }
    Ok(out)
}
