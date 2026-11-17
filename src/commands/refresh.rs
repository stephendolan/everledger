#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers = true, value_enum)]
    pub target: Option<Target>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Catalogue,
}
use crate::{Context, Result, commands::CommandOutput, document::Document, http::Payload};

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    if args.target.is_none() {
        context
            .diagnostics
            .warn("deprecated bare refresh; use: everledger refresh catalogue")?;
    }
    let catalogue = crate::catalogue_build::build(context)?;
    let mut used = vec![crate::catalogue::catalogue_source(
        &catalogue,
        context.clock,
    )?];
    let (vendor_sources, synced) = crate::vendors::refresh_cached(context)?;
    let has_vendor_sources = !vendor_sources.is_empty();
    used.extend(vendor_sources);
    let lines = crate::human::refresh::lines(synced.view());
    let mut code = crate::provenance::stale_code(&used);
    if synced.view().failed && !has_vendor_sources {
        code = crate::ExitCode::Unreachable;
    }
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
    output.stale = code == crate::ExitCode::Stale;
    Ok(output)
}
