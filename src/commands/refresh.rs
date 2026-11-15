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
    let synced = match crate::foreverdb::sync(context) {
        Ok(result) => result,
        Err(error) => {
            context
                .diagnostics
                .warn(&crate::human::refresh::retaining(&error))?;
            if crate::foreverdb::active_index(context)?.is_none() {
                return Err(error);
            }
            Payload::decode(
                Document::object([
                    ("configured", true.into()),
                    ("complete", false.into()),
                    ("failed", true.into()),
                ]),
                "ForeverDB sync",
            )?
        }
    };
    if let Some(index) = crate::foreverdb::active_index(context)? {
        used.extend(crate::foreverdb::index_sources(&index, context)?);
    }
    let lines = crate::human::refresh::lines(synced.view());
    let mut code = crate::provenance::stale_code(&used);
    if synced.view().failed && used.len() == 1 {
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
    Ok(output)
}
