#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(long = "item", allow_negative_numbers = true)]
    pub item: Option<String>,
    #[arg(long = "char", allow_negative_numbers = true)]
    pub char: Option<String>,
    #[arg(long = "since", allow_negative_numbers = true)]
    pub since: Option<String>,
    #[arg(long = "until", allow_negative_numbers = true)]
    pub until: Option<String>,
    #[arg(
        long = "venue",
        allow_negative_numbers = true,
        value_enum,
        default_value = "auction"
    )]
    pub venue: Option<crate::types::Venue>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{Context, Failure, Result, commands::CommandOutput};

pub fn run(_args: &Args, _context: &Context<'_>) -> Result<CommandOutput> {
    Err(Failure::error("sales: not yet ported"))
}
