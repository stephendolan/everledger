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
    #[arg(
        long = "char",
        allow_negative_numbers = true,
        help = "character scope; faction filtering unavailable"
    )]
    pub char: Option<String>,
    #[arg(
        long = "all",
        help = "all zones (also the default; no faction filtering)"
    )]
    pub all: bool,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{Context, Failure, Result, commands::CommandOutput};

pub fn run(_args: &Args, _context: &Context<'_>) -> Result<CommandOutput> {
    Err(Failure::error("vendors: not yet ported"))
}
