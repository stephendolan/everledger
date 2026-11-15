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
use crate::{Context, Failure, Result, commands::CommandOutput};

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    if args.target.is_none() {
        context
            .diagnostics
            .warn("deprecated bare refresh; use: everledger refresh catalogue")?;
    }
    Err(Failure::error("refresh: not yet ported"))
}
