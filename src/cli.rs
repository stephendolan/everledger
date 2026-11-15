use crate::{
    Context, ExitCode, Failure, Result,
    commands::{self, CommandOutput},
    document::{JsonMode, dumps},
};
use clap::{CommandFactory, FromArgMatches};
use std::{ffi::OsString, io::Write};
const EPILOG: &str = "Selectors: --char accepts a unique first or full character name.\nTimes: 7d, 24h, dates, local ISO timestamps, or UTC ISO with Z; since inclusive,\nuntil exclusive. Outcomes default to 7d; events to 24h. Limits: 50 human, 500 JSON.\nJSON uses API field names, integer copper totals and UTC ISO timestamps.\nHuman unknown values print as unknown; JSON keeps nulls and full provenance.\nHuman booleans print as yes/no; JSON keeps booleans.\nHuman output ends with one source: summary of sources, save/data times and ages.\nStale data has one stale: notice after all rows, before the source summary.\nOld journal saves show their age without a stale notice or exit-code change.\nJSON matched/returned counts describe the accompanying filtered array;\naggregates count all pages, events count the current page, bootstrap counts zero.\nExit codes: 0 answer, 1 error, 2 usage, 3 stale data printed, 4 source unreachable.\nJournal save age is normal: outcomes and net exit 0 even for old saves.\nPrice/margins AH freshness limit: 15m; catalogue advisory age: 14d.\nListings use the roster scan limit (15m). Positions apply both market/scan limits.\nVendor queries cache snapshots until refresh catalogue; after 1d they are stale.\nOn refresh failure a cache up to 7d old prints stale with exit 3, else exit 4.\nNinja sale/price claims are Classic 1.12 baseline; vendor sightings may be seen\nin Forever. ForeverDB maps its own sell-row labels separately from sightings.\nNinja quantity/faction are unavailable; ForeverDB retains stack and side.\nAll zones appear, including Horde; faction filtering is unavailable. Listed prices are not used in craft margins.\nConfiguration: standalone everledger binary.\nOnly connector transport needs python3 and the dynamic_credentials helper.\nEVERLEDGER_DATA_DIR defaults to ~/.everledger (catalogue and queried vendor cache).\nEVERLEDGER_ROSTER_URL defaults to http://localhost:8765; set EVERLEDGER_ROSTER_TOKEN\nor EVERLEDGER_ROSTER_TOKEN_FILE for bearer authentication.\nEVERLEDGER_MARKET defaults to forever.pvp.alliance.us.\nEVERLEDGER_AHLEDGER_URL defaults to https://api.ahledger.com.\nEVERLEDGER_VENDOR_URL defaults to https://wowforever.ninja.\nSet EVERLEDGER_FOREVERDB_API_KEY or EVERLEDGER_FOREVERDB_API_KEY_FILE\n(default: EVERLEDGER_DATA_DIR/foreverdb-api-key), or select vault transport with\nEVERLEDGER_FOREVERDB_VIA_CONNECTOR=custom.foreverdb (no raw key required).\nEVERLEDGER_DYNAMIC_CREDENTIALS_DIR defaults to\n/opt/muse/skills/custom-connector-setup/bin; connector mode needs dynamic_credentials.\nBoth transports send a browser User-Agent and refuse redirects.\nNinja stays active until a key or connector is configured and a sync completes.\nThen ForeverDB is automatic; failed syncs use the 7d stale-cache rule, with ninja\nretained as fallback. Data from ForeverDB.net (https://foreverdb.net).\nRefresh downloads profession pages and refreshes cached ninja IDs, or syncs\nForeverDB vendors only: at most 36 requests per 24h, paced below 30/minute.\nList pages use limit 100 and offset; 1,014 vendors need 11 list + 1,014 detail\nrequests = 1,025 per full sync, bootstrap and steady state (no sell revisions).\nAt 36/day this takes at least 29 daily attempts; retries add requests.\nRetryable HTTP 502 gets one retry after at least retry_after (minimum 60s),\nwithin the daily cap. Unfinished syncs resume; only complete indexes activate.\nForeverDB prices are per item only when stack price divides evenly by stack;\notherwise show the total and quantity, e.g. 10c per 3. Limited stock is marked.\nWith ForeverDB active, vendors adds a plain fallback-coverage count when the\nninja cache knows additional distinct sellers; --in applies to both sets.\nStatus names the transport and vendor/item record counts for each backend.\nStatus and vendor lookups never make ForeverDB requests.\nDeprecated for one release: verify (use status), bare refresh (use refresh catalogue),\nmargins --known NAME (use --char NAME). Warnings go to stderr; JSON stays stable.\nchar NAME auctions prints listings --char NAME on stderr and exits 2.\n";
#[derive(clap::Parser, Debug)]
#[command(name="everledger",about="everledger: WoW Forever prices, catalogue, vendors, characters and outcomes.",after_help=EPILOG,disable_help_subcommand=true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(clap::Subcommand, Debug, Clone)]
pub enum Command {
    #[command(name = "price", about = "AHledger quote plus vendor listed prices")]
    Price(crate::commands::price::Args),
    #[command(
        name = "recipe",
        about = "Catalogue facts and roster known-by annotation"
    )]
    Recipe(crate::commands::recipe::Args),
    #[command(name = "bridge", about = "Catalogue skill-up bands across a range")]
    Bridge(crate::commands::bridge::Args),
    #[command(
        name = "margins",
        about = "AHledger craft margins; optional roster recipe/level scope"
    )]
    Margins(crate::commands::margins::Args),
    #[command(
        name = "vendors",
        about = "Vendor listed prices/locations; ForeverDB includes faction"
    )]
    Vendors(crate::commands::vendors::Args),
    #[command(
        name = "char",
        about = "Characters, profession ranks and known recipes"
    )]
    Char(crate::commands::char::Args),
    #[command(
        name = "listings",
        about = "Recorded live auction listings and AHledger floor comparison"
    )]
    Listings(crate::commands::listings::Args),
    #[command(
        name = "positions",
        about = "Inventory holdings/listings and AHledger floor; basis unknown without assignments"
    )]
    Positions(crate::commands::positions::Args),
    #[command(name = "sales", about = "Sales proceeds (defaults to auction)")]
    Sales(crate::commands::sales::Args),
    #[command(name = "purchases", about = "Auction and vendor purchases")]
    Purchases(crate::commands::purchases::Args),
    #[command(
        name = "expirations",
        about = "Expired auctions with market repost inputs"
    )]
    Expirations(crate::commands::expirations::Args),
    #[command(
        name = "net",
        about = "Item auction cash/count diagnostic; profit basis stays unknown"
    )]
    Net(crate::commands::net::Args),
    #[command(
        name = "events",
        about = "Typed journal feed; prints next_cursor, has_more and service checkpoint"
    )]
    Events(crate::commands::events::Args),
    #[command(
        name = "status",
        about = "Reachability/freshness of AHledger, roster, outcomes, catalogue and vendors"
    )]
    Status(crate::commands::status::Args),
    #[command(
        name = "refresh",
        about = "Rebuild catalogue and refresh the configured vendor backend"
    )]
    Refresh(crate::commands::refresh::Args),
    #[command(
        name = "verify",
        about = "Deprecated catalogue anchor check (use status)"
    )]
    Verify(crate::commands::verify::Args),
}
impl Command {
    pub fn as_json(&self) -> bool {
        match self {
            Self::Price(args) => args.json,
            Self::Recipe(args) => args.json,
            Self::Bridge(args) => args.json,
            Self::Margins(args) => args.json,
            Self::Vendors(args) => args.json,
            Self::Char(args) => args.json,
            Self::Listings(args) => args.json,
            Self::Positions(args) => args.json,
            Self::Sales(args) => args.json,
            Self::Purchases(args) => args.json,
            Self::Expirations(args) => args.json,
            Self::Net(args) => args.json,
            Self::Events(args) => args.json,
            Self::Status(args) => args.json,
            Self::Refresh(args) => args.json,
            Self::Verify(args) => args.json,
        }
    }
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
pub fn dispatch(command: &Command, context: &Context<'_>) -> Result<CommandOutput> {
    match command {
        Command::Price(args) => commands::price::run(args, context),
        Command::Recipe(args) => commands::recipe::run(args, context),
        Command::Bridge(args) => commands::bridge::run(args, context),
        Command::Margins(args) => commands::margins::run(args, context),
        Command::Vendors(args) => commands::vendors::run(args, context),
        Command::Char(args) => commands::char::run(args, context),
        Command::Listings(args) => commands::listings::run(args, context),
        Command::Positions(args) => commands::positions::run(args, context),
        Command::Sales(args) => commands::sales::run(args, context),
        Command::Purchases(args) => commands::purchases::run(args, context),
        Command::Expirations(args) => commands::expirations::run(args, context),
        Command::Net(args) => commands::net::run(args, context),
        Command::Events(args) => commands::events::run(args, context),
        Command::Status(args) => commands::status::run(args, context),
        Command::Refresh(args) => commands::refresh::run(args, context),
        Command::Verify(args) => commands::verify::run(args, context),
    }
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
        Ok(output.code)
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
