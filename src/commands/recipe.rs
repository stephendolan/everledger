#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers=true,required=true,num_args=1..)]
    pub item: Vec<String>,
    #[arg(long="skill",value_parser=crate::cli::parse_integer,allow_negative_numbers=true)]
    pub skill: Option<i64>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{Context, Failure, Result, commands::CommandOutput};

pub fn run(_args: &Args, _context: &Context<'_>) -> Result<CommandOutput> {
    Err(Failure::error("recipe: not yet ported"))
}
