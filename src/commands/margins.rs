#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers = true)]
    pub profession: Option<String>,
    #[arg(long="level",value_parser=crate::cli::parse_integer,allow_negative_numbers=true)]
    pub level: Option<i64>,
    #[arg(long = "char", allow_negative_numbers = true)]
    pub char: Option<String>,
    #[arg(long = "known", allow_negative_numbers = true, hide = true)]
    pub known: Option<String>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{Context, Failure, Result, commands::CommandOutput};

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    if let Some(known) = &args.known {
        context.diagnostics.warn("deprecated --known; use --char")?;
        if args.char.as_ref().is_some_and(|name| name != known) {
            return Err(Failure::usage("--known conflicts with --char"));
        }
    }
    Err(Failure::error("margins: not yet ported"))
}
