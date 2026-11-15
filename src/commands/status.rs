#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{Context, Failure, Result, commands::CommandOutput};

pub fn run(_args: &Args, _context: &Context<'_>) -> Result<CommandOutput> {
    Err(Failure::error("status: not yet ported"))
}
