use clap::{Arg, ArgAction, Command};
pub fn parser() -> Command {
    let p=Command::new("everledger").disable_version_flag(true).subcommand_required(true).arg_required_else_help(false).args_override_self(true)
.about("everledger: WoW Forever prices, catalogue, vendors, characters and outcomes.").after_help("Selectors: --char accepts a unique first or full character name.\nTimes: 7d, 24h, dates, local ISO timestamps, or UTC ISO with Z; since inclusive,\nuntil exclusive. Outcomes default to 7d; events to 24h. Limits: 50 human, 500 JSON.\nJSON uses API field names, integer copper totals and UTC ISO timestamps.\nHuman unknown values print as unknown; JSON keeps nulls and full provenance.\nHuman booleans print as yes/no; JSON keeps booleans.\nHuman output ends with one source: summary of sources, save/data times and ages.\nStale data has one stale: notice after all rows, before the source summary.\nOld journal saves show their age without a stale notice or exit-code change.\nJSON matched/returned counts describe the accompanying filtered array;\naggregates count all pages, events count the current page, bootstrap counts zero.\nExit codes: 0 answer, 1 error, 2 usage, 3 stale data printed, 4 source unreachable.\nJournal save age is normal: outcomes and net exit 0 even for old saves.\nPrice/margins AH freshness limit: 15m; catalogue advisory age: 14d.\nListings use the roster scan limit (15m). Positions apply both market/scan limits.\nVendor queries cache snapshots until refresh catalogue; after 1d they are stale.\nOn refresh failure a cache up to 7d old prints stale with exit 3, else exit 4.\nNinja sale/price claims are Classic 1.12 baseline; vendor sightings may be seen\nin Forever. ForeverDB maps its own sell-row labels separately from sightings.\nNinja quantity/faction are unavailable; ForeverDB retains stack and side.\nAll zones appear, including Horde; faction filtering is unavailable. Listed prices are not used in craft margins.\nConfiguration: Python 3.11+ and curl; keep this executable beside everledger_*.py.\nEVERLEDGER_DATA_DIR defaults to ~/.everledger (catalogue and queried vendor cache).\nEVERLEDGER_ROSTER_URL defaults to http://localhost:8765; set EVERLEDGER_ROSTER_TOKEN\nor EVERLEDGER_ROSTER_TOKEN_FILE for bearer authentication.\nEVERLEDGER_MARKET defaults to forever.pvp.alliance.us.\nEVERLEDGER_AHLEDGER_URL defaults to https://api.ahledger.com.\nEVERLEDGER_VENDOR_URL defaults to https://wowforever.ninja.\nSet EVERLEDGER_FOREVERDB_API_KEY or EVERLEDGER_FOREVERDB_API_KEY_FILE\n(default: EVERLEDGER_DATA_DIR/foreverdb-api-key), or select vault transport with\nEVERLEDGER_FOREVERDB_VIA_CONNECTOR=custom.foreverdb (no raw key required).\nEVERLEDGER_DYNAMIC_CREDENTIALS_DIR defaults to\n/opt/muse/skills/custom-connector-setup/bin; connector mode needs dynamic_credentials.\nBoth transports send a browser User-Agent and refuse redirects.\nNinja stays active until a key or connector is configured and a sync completes.\nThen ForeverDB is automatic; failed syncs use the 7d stale-cache rule, with ninja\nretained as fallback. Data from ForeverDB.net (https://foreverdb.net).\nRefresh downloads profession pages and refreshes cached ninja IDs, or syncs\nForeverDB vendors only: at most 36 requests per 24h, paced below 30/minute.\nList pages use limit 100 and offset; 1,014 vendors need 11 list + 1,014 detail\nrequests = 1,025 per full sync, bootstrap and steady state (no sell revisions).\nAt 36/day this takes at least 29 daily attempts; retries add requests.\nRetryable HTTP 502 gets one retry after at least retry_after (minimum 60s),\nwithin the daily cap. Unfinished syncs resume; only complete indexes activate.\nForeverDB prices are per item only when stack price divides evenly by stack;\notherwise show the total and quantity, e.g. 10c per 3. Limited stock is marked.\nWith ForeverDB active, vendors adds a plain fallback-coverage count when the\nninja cache knows additional distinct sellers; --in applies to both sets.\nStatus names the transport and vendor/item record counts for each backend.\nStatus and vendor lookups never make ForeverDB requests.\nDeprecated for one release: verify (use status), bare refresh (use refresh catalogue),\nmargins --known NAME (use --char NAME). Warnings go to stderr; JSON stays stable.\nchar NAME auctions prints listings --char NAME on stderr and exits 2.\n");
    let mut p = p;
    let c = Command::new("price")
        .about("AHledger quote plus vendor listed prices")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("item")
            .index(1)
            .allow_negative_numbers(true)
            .num_args(1..)
            .required(true),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("recipe")
        .about("Catalogue facts and roster known-by annotation")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("item")
            .index(1)
            .allow_negative_numbers(true)
            .num_args(1..)
            .required(true),
    );
    c = c.arg(
        Arg::new("skill")
            .long("skill")
            .allow_negative_numbers(true)
            .value_parser(clap::value_parser!(crate::types::CliInteger)),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("bridge")
        .about("Catalogue skill-up bands across a range")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("profession")
            .index(1)
            .allow_negative_numbers(true)
            .required(true),
    );
    c = c.arg(
        Arg::new("start")
            .index(2)
            .allow_negative_numbers(true)
            .value_parser(clap::value_parser!(crate::types::CliInteger))
            .required(true),
    );
    c = c.arg(
        Arg::new("end")
            .index(3)
            .allow_negative_numbers(true)
            .value_parser(clap::value_parser!(crate::types::CliInteger))
            .required(true),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("margins")
        .about("AHledger craft margins; optional roster recipe/level scope")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("profession")
            .index(1)
            .allow_negative_numbers(true)
            .required(false),
    );
    c = c.arg(
        Arg::new("level")
            .long("level")
            .allow_negative_numbers(true)
            .value_parser(clap::value_parser!(crate::types::CliInteger)),
    );
    c = c.arg(Arg::new("char").long("char").allow_negative_numbers(true));
    c = c.arg(
        Arg::new("known")
            .long("known")
            .allow_negative_numbers(true)
            .hide(true),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("vendors")
        .about("Vendor listed prices/locations; ForeverDB includes faction")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("item")
            .index(1)
            .allow_negative_numbers(true)
            .num_args(1..)
            .required(true),
    );
    c = c.arg(
        Arg::new("zone")
            .long("in")
            .allow_negative_numbers(true)
            .help("zone/city substring; aliases IF, SW, Darn, Org, TB, UC"),
    );
    c = c.arg(
        Arg::new("char")
            .long("char")
            .allow_negative_numbers(true)
            .help("character scope; faction filtering unavailable"),
    );
    c = c.arg(
        Arg::new("all")
            .long("all")
            .action(ArgAction::SetTrue)
            .help("all zones (also the default; no faction filtering)"),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("char")
        .about("Characters, profession ranks and known recipes")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("name")
            .index(1)
            .allow_negative_numbers(true)
            .required(false),
    );
    c = c.arg(
        Arg::new("profession")
            .index(2)
            .allow_negative_numbers(true)
            .required(false),
    );
    c = c.arg(
        Arg::new("skill")
            .long("skill")
            .allow_negative_numbers(true)
            .value_parser(clap::value_parser!(crate::types::CliInteger)),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("listings")
        .about("Recorded live auction listings and AHledger floor comparison")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(Arg::new("char").long("char").allow_negative_numbers(true));
    c = c.arg(Arg::new("item").long("item").allow_negative_numbers(true));
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("positions")
        .about("Inventory holdings/listings and AHledger floor; basis unknown without assignments")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(Arg::new("char").long("char").allow_negative_numbers(true));
    c = c.arg(Arg::new("item").long("item").allow_negative_numbers(true));
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("sales")
        .about("Sales proceeds (defaults to auction)")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(Arg::new("item").long("item").allow_negative_numbers(true));
    c = c.arg(Arg::new("char").long("char").allow_negative_numbers(true));
    c = c.arg(Arg::new("since").long("since").allow_negative_numbers(true));
    c = c.arg(Arg::new("until").long("until").allow_negative_numbers(true));
    c = c.arg(
        Arg::new("venue")
            .long("venue")
            .allow_negative_numbers(true)
            .value_parser(["auction", "vendor"])
            .default_value("auction"),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("purchases")
        .about("Auction and vendor purchases")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(Arg::new("item").long("item").allow_negative_numbers(true));
    c = c.arg(Arg::new("char").long("char").allow_negative_numbers(true));
    c = c.arg(Arg::new("since").long("since").allow_negative_numbers(true));
    c = c.arg(Arg::new("until").long("until").allow_negative_numbers(true));
    c = c.arg(
        Arg::new("venue")
            .long("venue")
            .allow_negative_numbers(true)
            .value_parser(["auction", "vendor"]),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("expirations")
        .about("Expired auctions with market repost inputs")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(Arg::new("item").long("item").allow_negative_numbers(true));
    c = c.arg(Arg::new("char").long("char").allow_negative_numbers(true));
    c = c.arg(Arg::new("since").long("since").allow_negative_numbers(true));
    c = c.arg(Arg::new("until").long("until").allow_negative_numbers(true));
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("net")
        .about("Item auction cash/count diagnostic; profit basis stays unknown")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("item")
            .index(1)
            .allow_negative_numbers(true)
            .num_args(1..)
            .required(true),
    );
    c = c.arg(Arg::new("char").long("char").allow_negative_numbers(true));
    c = c.arg(Arg::new("since").long("since").allow_negative_numbers(true));
    c = c.arg(Arg::new("until").long("until").allow_negative_numbers(true));
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("events")
        .about("Typed journal feed; prints next_cursor, has_more and service checkpoint")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(Arg::new("item").long("item").allow_negative_numbers(true));
    c = c.arg(Arg::new("char").long("char").allow_negative_numbers(true));
    c = c.arg(Arg::new("since").long("since").allow_negative_numbers(true));
    c = c.arg(Arg::new("until").long("until").allow_negative_numbers(true));
    c = c.arg(
        Arg::new("venue")
            .long("venue")
            .allow_negative_numbers(true)
            .value_parser(["auction", "vendor"]),
    );
    c = c.arg(
        Arg::new("type")
            .long("type")
            .allow_negative_numbers(true)
            .help("comma-separated purchase, sale, posting, expiration, cancellation"),
    );
    c = c.arg(
        Arg::new("cursor")
            .long("cursor")
            .allow_negative_numbers(true)
            .help("snapshot page cursor from next_cursor; inherits fixed filters"),
    );
    c = c.arg(
        Arg::new("after")
            .long("after")
            .allow_negative_numbers(true)
            .help("terminal after_cursor/checkpoint; exclusive with since/until"),
    );
    c = c.arg(
        Arg::new("limit")
            .long("limit")
            .allow_negative_numbers(true)
            .value_parser(clap::value_parser!(crate::types::CliInteger))
            .help("0 bootstraps a watch; default 50 human, 500 JSON"),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    c=c.after_help("Start a watch: everledger events --limit 0 --json. Bootstrap drains a snapshot with positive service limits and prints no entries. Save after_cursor (the service terminal checkpoint) only after has_more is false. While has_more is true, pass next_cursor to --cursor to finish the snapshot; page cursors retain the local view. Character/item/type/venue filters are local views over an unfiltered checkpoint and may change between polls. Limit bounds the scanned page, so a filtered page may be empty with has_more true. Times are sent as integer epoch seconds. On 409 cursor_invalid, alert once and resync from resync_since; retries can replay entries. snapshot_changed requires restarting from the last committed checkpoint. Act before saving progress for at-least-once delivery. This CLI persists nothing.");
    p = p.subcommand(c);
    let c = Command::new("status")
        .about("Reachability/freshness of AHledger, roster, outcomes, catalogue and vendors")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("refresh")
        .about("Rebuild catalogue and refresh the configured vendor backend")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("target")
            .index(1)
            .allow_negative_numbers(true)
            .required(false)
            .value_parser(["catalogue"]),
    );
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    let c = Command::new("verify")
        .about("Deprecated catalogue anchor check (use status)")
        .args_override_self(true);
    let mut c = c;
    c = c.arg(
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("one JSON document using API field names"),
    );
    p = p.subcommand(c);
    p
}
