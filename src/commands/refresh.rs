#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers = true, value_enum)]
    pub target: Target,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Catalogue,
}
use crate::{Context, Result, commands::CommandOutput, document::Document, http::Payload};

pub fn run(_args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let catalogue = crate::catalogue_build::build(context)?;
    let mut used = vec![crate::catalogue::catalogue_source(
        &catalogue,
        context.clock,
    )?];
    let (vendor_sources, synced) = crate::vendors::refresh_cached(context)?;
    let stale =
        !synced.view().failed && vendor_sources.iter().any(|s| s.view().stale == Some(true));
    used.extend(vendor_sources);
    let lines = crate::human::refresh::lines(synced.view());
    let code = if synced.view().failed {
        crate::ExitCode::Unreachable
    } else if stale {
        crate::ExitCode::Stale
    } else {
        crate::ExitCode::Answer
    };
    let data = Document::object([
        ("refreshed", "catalogue".into()),
        ("vendor_sync", synced.into_document()),
        (
            "sources",
            Document::Array(used.into_iter().map(Payload::into_document).collect()),
        ),
    ]);
    let mut output = CommandOutput::new(data, lines);
    output.code = code;
    output.stale = stale;
    Ok(output)
}
