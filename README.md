# everledger

everledger is a standalone Rust CLI for WoW: Forever prices, crafting, vendors,
characters and journal outcomes. This README is its command reference, generated
verbatim from the binary's `--help`.

See [Journalator consumer notes](docs/journalator-contract.md) for watch checkpoints,
local filter views, and the live-service regression evidence.

```text
everledger: WoW Forever prices, catalogue, vendors, characters and outcomes.

Usage: everledger <COMMAND>

Commands:
  price        AHledger quote plus vendor listed prices
  recipe       Catalogue facts and roster known-by annotation
  bridge       Catalogue skill-up bands across a range
  margins      AHledger craft margins; optional roster recipe/level scope
  vendors      Vendor listed prices/locations; ForeverDB includes faction
  char         Characters, profession ranks and known recipes
  listings     Recorded live auction listings and AHledger floor comparison
  positions    Inventory holdings/listings and AHledger floor; basis unknown without assignments
  sales        Sales proceeds (defaults to auction)
  purchases    Auction and vendor purchases
  expirations  Expired auctions with market repost inputs
  net          Item auction cash/count diagnostic; profit basis stays unknown
  events       Typed journal feed; prints next_cursor, has_more and service checkpoint
  status       Reachability/freshness of AHledger, roster, outcomes, catalogue and vendors
  refresh      Rebuild catalogue and refresh the configured vendor backend
  verify       Deprecated catalogue anchor check (use status)

Options:
  -h, --help  Print help

Selectors: --char accepts a unique first or full character name.
Times: 7d, 24h, dates, local ISO timestamps, or UTC ISO with Z; since inclusive,
until exclusive. Outcomes default to 7d; events to 24h. Limits: 50 human, 500 JSON.
JSON uses API field names, integer copper totals and UTC ISO timestamps.
Human unknown values print as unknown; JSON keeps nulls and full provenance.
Human booleans print as yes/no; JSON keeps booleans.
Human output ends with one source: summary of sources, save/data times and ages.
Stale data has one stale: notice after all rows, before the source summary.
Old journal saves show their age without a stale notice or exit-code change.
JSON matched/returned counts describe the accompanying filtered array;
aggregates count all pages, events count the current page, bootstrap counts zero.
Exit codes: 0 answer, 1 error, 2 usage, 3 stale data printed, 4 source unreachable.
Journal save age is normal: outcomes and net exit 0 even for old saves.
Price/margins AH freshness limit: 15m; catalogue advisory age: 14d.
Listings use the roster scan limit (15m). Positions apply both market/scan limits.
Vendor queries cache snapshots until refresh catalogue; after 1d they are stale.
On refresh failure a cache up to 7d old prints stale with exit 3, else exit 4.
Ninja sale/price claims are Classic 1.12 baseline; vendor sightings may be seen
in Forever. ForeverDB maps its own sell-row labels separately from sightings.
Ninja quantity/faction are unavailable; ForeverDB retains stack and side.
All zones appear, including Horde; faction filtering is unavailable. Listed prices are not used in craft margins.
Configuration: standalone everledger binary.
Only connector transport needs python3 and the dynamic_credentials helper.
EVERLEDGER_DATA_DIR defaults to ~/.everledger (catalogue and queried vendor cache).
EVERLEDGER_ROSTER_URL defaults to http://localhost:8765; set EVERLEDGER_ROSTER_TOKEN
or EVERLEDGER_ROSTER_TOKEN_FILE for bearer authentication.
EVERLEDGER_MARKET defaults to forever.pvp.alliance.us.
EVERLEDGER_AHLEDGER_URL defaults to https://api.ahledger.com.
EVERLEDGER_VENDOR_URL defaults to https://wowforever.ninja.
Set EVERLEDGER_FOREVERDB_API_KEY or EVERLEDGER_FOREVERDB_API_KEY_FILE
(default: EVERLEDGER_DATA_DIR/foreverdb-api-key), or select vault transport with
EVERLEDGER_FOREVERDB_VIA_CONNECTOR=custom.foreverdb (no raw key required).
EVERLEDGER_DYNAMIC_CREDENTIALS_DIR defaults to
/opt/muse/skills/custom-connector-setup/bin; connector mode needs dynamic_credentials.
Both transports send a browser User-Agent and refuse redirects.
Ninja stays active until a key or connector is configured and a sync completes.
Then ForeverDB is automatic; failed syncs use the 7d stale-cache rule, with ninja
retained as fallback. Data from ForeverDB.net (https://foreverdb.net).
Refresh downloads profession pages and refreshes cached ninja IDs, or syncs
ForeverDB vendors only: default cap 36 requests per 24h, paced below 30/minute.
Override with EVERLEDGER_FOREVERDB_DAILY_REQUESTS (a positive integer).
List pages use limit 100 and offset; today's 1,014 vendors need 11 list + 1,014
detail requests = 1,025 per full sync, bootstrap and steady state (no sell revisions).
At the default 36/day, bootstrap takes at least 29 daily attempts; retries add requests.
Retryable HTTP 502 gets one retry after at least retry_after (minimum 60s),
within the daily cap. Unfinished syncs resume; only complete indexes activate.
ForeverDB prices are per item only when stack price divides evenly by stack;
otherwise show the total and quantity, e.g. 10c per 3. Limited stock is marked.
With ForeverDB active, vendors adds a plain fallback-coverage count when the
ninja cache knows additional distinct sellers; --in applies to both sets.
Status names the transport and vendor/item record counts for each backend.
Status and vendor lookups never make ForeverDB requests.
Deprecated for one release: verify (use status), bare refresh (use refresh catalogue),
margins --known NAME (use --char NAME). Warnings go to stderr; JSON stays stable.
char NAME auctions prints listings --char NAME on stderr and exits 2.
```

## price

```text
AHledger quote plus vendor listed prices

Usage: everledger price [OPTIONS] <ITEM>...

Arguments:
  <ITEM>...  

Options:
      --json  one JSON document using API field names
  -h, --help  Print help
```

## recipe

```text
Catalogue facts and roster known-by annotation

Usage: everledger recipe [OPTIONS] <ITEM>...

Arguments:
  <ITEM>...  

Options:
      --skill <SKILL>  
      --json           one JSON document using API field names
  -h, --help           Print help
```

## bridge

```text
Catalogue skill-up bands across a range

Usage: everledger bridge [OPTIONS] <PROFESSION> <START> <END>

Arguments:
  <PROFESSION>  
  <START>       
  <END>         

Options:
      --json  one JSON document using API field names
  -h, --help  Print help
```

## margins

```text
AHledger craft margins; optional roster recipe/level scope

Usage: everledger margins [OPTIONS] [PROFESSION]

Arguments:
  [PROFESSION]  

Options:
      --level <LEVEL>  
      --char <CHAR>    
      --json           one JSON document using API field names
  -h, --help           Print help
```

## vendors

```text
Vendor listed prices/locations; ForeverDB includes faction

Usage: everledger vendors [OPTIONS] <ITEM>...

Arguments:
  <ITEM>...  

Options:
      --in <ZONE>    zone/city substring; aliases IF, SW, Darn, Org, TB, UC
      --char <CHAR>  character scope; faction filtering unavailable
      --all          all zones (also the default; no faction filtering)
      --json         one JSON document using API field names
  -h, --help         Print help
```

## char

```text
Characters, profession ranks and known recipes

Usage: everledger char [OPTIONS] [NAME] [PROFESSION]

Arguments:
  [NAME]        
  [PROFESSION]  

Options:
      --skill <SKILL>  
      --json           one JSON document using API field names
  -h, --help           Print help
```

## listings

```text
Recorded live auction listings and AHledger floor comparison

Usage: everledger listings [OPTIONS]

Options:
      --char <CHAR>  
      --item <ITEM>  
      --json         one JSON document using API field names
  -h, --help         Print help
```

## positions

```text
Inventory holdings/listings and AHledger floor; basis unknown without assignments

Usage: everledger positions [OPTIONS]

Options:
      --char <CHAR>  
      --item <ITEM>  
      --json         one JSON document using API field names
  -h, --help         Print help
```

## sales

```text
Sales proceeds (defaults to auction)

Usage: everledger sales [OPTIONS]

Options:
      --item <ITEM>    
      --char <CHAR>    
      --since <SINCE>  
      --until <UNTIL>  
      --venue <VENUE>  [default: auction] [possible values: auction, vendor]
      --json           one JSON document using API field names
  -h, --help           Print help
```

## purchases

```text
Auction and vendor purchases

Usage: everledger purchases [OPTIONS]

Options:
      --item <ITEM>    
      --char <CHAR>    
      --since <SINCE>  
      --until <UNTIL>  
      --venue <VENUE>  [possible values: auction, vendor]
      --json           one JSON document using API field names
  -h, --help           Print help
```

## expirations

```text
Expired auctions with market repost inputs

Usage: everledger expirations [OPTIONS]

Options:
      --item <ITEM>    
      --char <CHAR>    
      --since <SINCE>  
      --until <UNTIL>  
      --json           one JSON document using API field names
  -h, --help           Print help
```

## net

```text
Item auction cash/count diagnostic; profit basis stays unknown

Usage: everledger net [OPTIONS] <ITEM>...

Arguments:
  <ITEM>...  

Options:
      --char <CHAR>    
      --since <SINCE>  
      --until <UNTIL>  
      --json           one JSON document using API field names
  -h, --help           Print help
```

## events

```text
Typed journal feed; prints next_cursor, has_more and service checkpoint

Usage: everledger events [OPTIONS]

Options:
      --item <ITEM>        
      --char <CHAR>        
      --since <SINCE>      
      --until <UNTIL>      
      --venue <VENUE>      [possible values: auction, vendor]
      --type <EVENT_TYPE>  comma-separated purchase, sale, posting, expiration, cancellation
      --cursor <CURSOR>    snapshot page cursor from next_cursor; inherits fixed filters
      --after <AFTER>      terminal after_cursor/checkpoint; exclusive with since/until
      --limit <LIMIT>      0 bootstraps a watch; default 50 human, 500 JSON
      --json               one JSON document using API field names
  -h, --help               Print help

Start a watch: everledger events --limit 0 --json. Bootstrap drains a snapshot
with positive service limits and prints no entries. Save after_cursor (the
service terminal checkpoint) only after has_more is false. While has_more is
true, pass next_cursor to --cursor to finish the snapshot; page cursors retain
the local view. Character/item/type/venue filters are local views over an
unfiltered checkpoint and may change between polls. Limit bounds the scanned
page, so a filtered page may be empty with has_more true. Times are sent as
integer epoch seconds. On 409 cursor_invalid, alert once and resync from
resync_since; retries can replay entries. snapshot_changed requires restarting
from the last committed checkpoint. Act before saving progress for
at-least-once delivery. This CLI persists nothing.
```

## status

```text
Reachability/freshness of AHledger, roster, outcomes, catalogue and vendors

Usage: everledger status [OPTIONS]

Options:
      --json  one JSON document using API field names
  -h, --help  Print help
```

## refresh

```text
Rebuild catalogue and refresh the configured vendor backend

Usage: everledger refresh [OPTIONS] [TARGET]

Arguments:
  [TARGET]  [possible values: catalogue]

Options:
      --json  one JSON document using API field names
  -h, --help  Print help
```

## verify

```text
Deprecated catalogue anchor check (use status)

Usage: everledger verify [OPTIONS]

Options:
      --json  one JSON document using API field names
  -h, --help  Print help
```

## ForeverDB backend

Ninja is the default until a key or connector is configured and a full ForeverDB
sync completes. Run `everledger refresh catalogue`; unfinished work resumes on
later daily refreshes. The mapping follows the API shapes probed November 15,
2026. Both bootstrap and steady state require 1,025 requests at today's 1,014
vendors. At the default cap of 36 requests per 24 hours, bootstrap takes at least
29 daily attempts. Set `EVERLEDGER_FOREVERDB_DAILY_REQUESTS` to a positive integer
to override the cap, for example `5000` for Stephen's chosen free-tier limit.
Invalid values are usage errors. Pacing stays at 2.1 seconds between requests,
and retries count against the configured cap. No per-vendor sell revision is
available.
See the [backend report and exact connector setup commands](docs/foreverdb.md)
for mapping, price/stack rules, request counts, coverage comparison and fixtures.

ForeverDB results disclose additional distinct sellers already in the ninja
cache in one plain line, scoped by `--in` when supplied. Status reports vendor
and item record counts for both backends and the selected transport.

## Build and verification

Build the release binary and run the Rust tests:

```sh
cargo build --release
cargo test
```

Regenerate the command reference from the built binary:

```sh
python3 scripts/update_command_reference.py target/release/everledger
```

The Rust tests check every reference block against the binary's help. See
[the parity harness](parity/README.md) for differential verification against the
Python oracle, argument-surface checks, and the isolated fixture setup;
[the architecture notes](docs/architecture.md) for the crate's conventions; and
[the register of differences](docs/rust-port-differences.md) for every place the
binary deliberately departs from the Python reference.

## Credits

- **Prices from AHledger** (https://ahledger.com). All auction house prices, listing counts, and sales estimates come from AHledger's API for the `forever.pvp.alliance.us` market.
- **Recipe data from wowforever.ninja** (https://wowforever.ninja), which publishes WoW: Forever client data. Its vendor and world data derive in part from the cMaNGOS classic-db baseline and are GPL-3.0 with attribution; the catalogue built by this tool inherits that. Treat baseline rows as unconfirmed until checked against the live game: Forever moves vendors and sources.
- **Data from [ForeverDB.net](https://foreverdb.net)**. The optional ForeverDB adapter indexes vendor data extracted from the Forever client. It activates after a key or connector is configured and a complete sync finishes; wire shapes follow the November 15, 2026 probes.

World of Warcraft and WoW: Forever are not affiliated with this project. This is a personal, non-commercial tool.
