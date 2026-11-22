use crate::{
    Context, ExitCode, Failure, Result,
    commands::{self, CommandOutput},
    document::{JsonMode, dumps},
};
use clap::{CommandFactory, FromArgMatches};
use std::{ffi::OsString, io::Write};
#[derive(clap::Parser, Debug)]
#[command(name="everledger",about="everledger: WoW Forever prices, catalogue, vendors, characters and outcomes.",after_help=help_epilogue(),disable_help_subcommand=true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(clap::Args, Debug, Clone, Default)]
pub struct OutputArgs {
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}

macro_rules! commands {
    ($( $(#[$meta:meta])* $variant:ident($module:ident), )*) => {
        #[derive(clap::Subcommand, Debug, Clone)]
        pub enum Command {
            $( $(#[$meta])* $variant(commands::$module::Args), )*
        }
        impl Command {
            pub fn as_json(&self) -> bool {
                match self {
                    $( Self::$variant(args) => args.output.json, )*
                }
            }
        }
        pub fn dispatch(command: &Command, context: &Context<'_>) -> Result<CommandOutput> {
            match command {
                $( Command::$variant(args) => commands::$module::run(args, context), )*
            }
        }
    };
}

commands! {
    #[command(name = "price", about = "AHledger quote plus vendor listed prices")]
    Price(price),
    #[command(
        name = "recipe",
        about = "Catalogue facts and roster known-by annotation"
    )]
    Recipe(recipe),
    #[command(name = "bridge", about = "Catalogue skill-up bands across a range")]
    Bridge(bridge),
    #[command(
        name = "margins",
        about = "AHledger craft margins; optional roster recipe/level scope"
    )]
    Margins(margins),
    #[command(name = "parity", about = "Catalogue and item-use conversions at AHledger floors and medians")]
    Parity(parity),
    #[command(
        name = "vendors",
        about = "Vendor listed prices/locations; ForeverDB includes faction"
    )]
    Vendors(vendors),
    #[command(
        name = "char",
        about = "Characters, profession ranks, known recipes and saved RestedXP position"
    )]
    Char(char),
    #[command(name = "guide", about = "RestedXP steps, search and installed guide list")]
    Guide(guide),
    #[command(
        name = "listings",
        about = "Recorded live auction listings and AHledger floor comparison"
    )]
    Listings(listings),
    #[command(
        name = "positions",
        about = "Inventory holdings/listings and AHledger floor; basis unknown without assignments"
    )]
    Positions(positions),
    #[command(name = "sales", about = "Sales proceeds (defaults to auction)")]
    Sales(sales),
    #[command(name = "purchases", about = "Auction and vendor purchases")]
    Purchases(purchases),
    #[command(
        name = "expirations",
        about = "Expired auctions with market repost inputs"
    )]
    Expirations(expirations),
    #[command(
        name = "net",
        about = "Item auction cash/count diagnostic; profit basis stays unknown"
    )]
    Net(net),
    #[command(
        name = "events",
        about = "Typed journal feed; prints next_cursor, has_more and service checkpoint"
    )]
    Events(events),
    #[command(
        name = "status",
        about = "Reachability/freshness of AHledger, roster, outcomes, catalogue and vendors"
    )]
    Status(status),
    #[command(
        name = "refresh",
        about = "Rebuild catalogue and refresh the configured vendor backend"
    )]
    Refresh(refresh),
}

fn help_epilogue() -> String {
    format!(
        include_str!("cli_help.txt"),
        market_minutes = crate::market::FRESHNESS_SECONDS / 60,
        catalogue_days = crate::catalogue::ADVISORY_DAYS,
        data_dir_name = crate::config::DATA_DIR_NAME,
        roster_url = crate::config::DEFAULT_ROSTER_URL,
        market = crate::config::DEFAULT_MARKET,
        ahledger_url = crate::config::DEFAULT_AHLEDGER_URL,
        vendor_url = crate::config::DEFAULT_VENDOR_URL,
        key_file = crate::config::DEFAULT_KEY_FILE,
        dynamic_credentials_dir = crate::config::DEFAULT_DYNAMIC_CREDENTIALS_DIR,
        daily_cap = crate::config::DEFAULT_DAILY_REQUESTS,
    )
}
pub fn parser() -> clap::Command {
    Cli::command().term_width(80).disable_colored_help(true)
}
pub fn parse_integer(text: &str) -> std::result::Result<i64, String> {
    let text = text.trim();
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'_')
    {
        return Err("expected an ASCII integer in i64 range".into());
    }
    text.replace('_', "")
        .parse()
        .map_err(|_| "integer outside i64 range".into())
}
pub fn parse_env() -> std::result::Result<Command, clap::Error> {
    parse_from(std::env::args_os())
}
pub fn parse_from(
    args: impl IntoIterator<Item = impl Into<OsString>>,
) -> std::result::Result<Command, clap::Error> {
    let mut argv = args.into_iter().map(Into::into).collect::<Vec<OsString>>();
    if argv.get(1).is_some_and(|s| s == "--") {
        argv.remove(1);
    }
    let mut definition = parser();
    let matches = definition.try_get_matches_from_mut(&argv)?;
    if let Some((name, sub)) = matches.subcommand()
        && let Some(command) = definition.find_subcommand(name)
    {
        if command.get_positionals().next().is_none() && argv.iter().skip(2).any(|s| s == "--") {
            return Err(definition.error(clap::error::ErrorKind::UnknownArgument, "unexpected --"));
        }
        for arg in command.get_positionals() {
            if arg.get_num_args().is_some_and(|n| n.max_values() > 1)
                && let Some(indices) = sub.indices_of(arg.get_id().as_str())
            {
                let indices = indices.collect::<Vec<_>>();
                if indices.windows(2).any(|w| w[1] != w[0] + 1) {
                    return Err(parser().error(
                        clap::error::ErrorKind::UnknownArgument,
                        "positional values must be contiguous",
                    ));
                }
            }
        }
    }
    Cli::from_arg_matches(&matches).map(|cli| cli.command)
}
pub fn execute(command: &Command, context: &Context<'_>, stdout: &mut dyn Write) -> ExitCode {
    let result = dispatch(command, context).and_then(|output| {
        let rendered = crate::human::emit(
            &output.data,
            &output.lines,
            command.as_json(),
            output.stale,
            &output.source_families,
            context.clock,
        )?;
        stdout.write_all(rendered.as_bytes())?;
        Ok(output.exit_code())
    });
    match result {
        Ok(code) => code,
        Err(error) => {
            let _ = write_failure(&error, context.diagnostics);
            error.code
        }
    }
}
pub fn write_failure(
    error: &Failure,
    diagnostics: &dyn crate::diagnostics::Diagnostics,
) -> Result<()> {
    diagnostics.warn(&error.message)?;
    if let Some(response) = error
        .body
        .as_ref()
        .and_then(|b| b.get("response"))
        .filter(|v| v.truthy())
    {
        diagnostics.warn(&dumps(response, JsonMode::Error))?;
    }
    Ok(())
}
