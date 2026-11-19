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
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}
use crate::{
    Context, Result,
    commands::CommandOutput,
    vendors::{self, VendorAnswer},
};
pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let item = crate::catalogue::item_for(&args.item.join(" "), context)?;
    let mut answer = vendors::lookup(item.id, context)?;
    match &mut answer {
        VendorAnswer::Ninja(a) => {
            if let Some(q) = args.zone.as_deref().filter(|s| !s.is_empty()) {
                a.select_zone(q)?;
            }
        }
        VendorAnswer::Foreverdb(a) => {
            if let Some(q) = args.zone.as_deref().filter(|s| !s.is_empty()) {
                a.select_zone(q)?;
            }
        }
    }
    let mut data = match &answer {
        VendorAnswer::Ninja(a) => a.public(),
        VendorAnswer::Foreverdb(a) => a.public(),
    };
    crate::catalogue::disclose_item_source(&mut data, Some(&item), context)?;
    let lines = crate::human::vendors::render(&answer);
    let code = crate::provenance::stale_code(&crate::provenance::sources(&data)?);
    let mut out = CommandOutput::new(data, lines);
    out.code = code;
    out.stale = code == crate::ExitCode::Stale;
    Ok(out)
}
