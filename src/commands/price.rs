use crate::{Context, Failure, Result, commands::CommandOutput};

pub fn run(_args: &clap::ArgMatches, _context: &Context<'_>) -> Result<CommandOutput> {
    Err(Failure::error("price: not yet ported"))
}
