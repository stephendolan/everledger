# Command reference

Generated from `--help`; regenerate with `python3 scripts/update_command_reference.py target/release/everledger`.

```text
everledger: WoW Forever prices, catalogue, vendors, characters and outcomes.

Usage: everledger <COMMAND>

Commands:
  price        AHledger quote plus vendor listed prices
  recipe       Catalogue facts and roster known-by annotation
  bridge       Catalogue skill-up bands across a range
  margins      AHledger craft margins; optional roster recipe/level scope
  parity       Catalogue and item-use conversions at AHledger floors and medians
  vendors      Vendor listed prices/locations; ForeverDB includes faction
  char         Characters, profession ranks, known recipes and saved RestedXP position
  guide        RestedXP steps, search and installed guide list
  listings     Recorded live auction listings and AHledger floor comparison
  positions    Inventory holdings/listings and AHledger floor; basis unknown without assignments
  sales        Sales proceeds (defaults to auction)
  saleslog     First-party listings, outcomes and execution analysis
  purchases    Auction and vendor purchases
  expirations  Expired auctions with market repost inputs
  net          Item auction cash/count diagnostic; profit basis stays unknown
  events       Typed journal feed; prints next_cursor, has_more and service checkpoint
  status       Reachability/freshness of AHledger, roster, sales log, catalogue and vendors
  refresh      Rebuild catalogue and refresh the configured vendor backend

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
Journal save age is normal: old saves alone do not change exit codes.
Price/margins/parity AH freshness limit: 15m; catalogue advisory age: 14d.
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
ForeverDB vendors only: rolling 24-hour request budget across attempts.
Default cap 36 requests; paced below 30/minute.
Override with EVERLEDGER_FOREVERDB_DAILY_REQUESTS (a positive integer).
List pages use limit 100 and offset; today's 1,014 vendors need 11 list + 1,014
detail requests = 1,025 per full sync, bootstrap and steady state (no sell revisions).
At the default 36/day, bootstrap takes at least 29 daily attempts; retries add requests.
Retryable HTTP 502 gets one retry within the daily cap; in-attempt waits are
60-300s. Longer waits stay pending until a later attempt. Unfinished syncs resume; only complete indexes activate.
ForeverDB prices are per item only when stack price divides evenly by stack;
otherwise show the total and quantity, e.g. 10c per 3. Limited stock is marked.
With ForeverDB active, vendors adds a plain fallback-coverage count when the
ninja cache knows additional distinct sellers; --in applies to both sets.
Status names the transport and vendor/item record counts for each backend.
Status and vendor lookups never make ForeverDB requests.
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

## parity

```text
Catalogue and item-use conversions at AHledger floors and medians

Usage: everledger parity [OPTIONS]

Options:
      --all   Include conversions with no positive spread
      --json  one JSON document using API field names
  -h, --help  Print help
```

## vendors

```text
Vendor listed prices/locations; ForeverDB includes faction

Usage: everledger vendors [OPTIONS] <ITEM>...

Arguments:
  <ITEM>...  

Options:
      --in <ZONE>  zone/city substring; aliases IF, SW, Darn, Org, TB, UC
      --json       one JSON document using API field names
  -h, --help       Print help
```

## char

```text
Characters, profession ranks, known recipes and saved RestedXP position

Usage: everledger char [OPTIONS] [NAME] [PROFESSION]

Arguments:
  [NAME]        
  [PROFESSION]  

Options:
      --skill <SKILL>  
      --json           one JSON document using API field names
  -h, --help           Print help
```

## guide

```text
RestedXP steps, search and installed guide list

Usage: everledger guide [OPTIONS] [NAME]

Arguments:
  [NAME]  Character name or unique first name

Options:
      --list           List installed guide families and chapters
      --group <GROUP>  Filter the list by family or subgroup
      --search <TERM>  Search chapters, step text and targets
      --limit <LIMIT>  Maximum results (default 5 upcoming or 20 matches)
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

## saleslog

```text
First-party listings, outcomes and execution analysis

Usage: everledger saleslog [OPTIONS] <COMMAND>

Commands:
  listings  Inspect first-party postings and listing-time scan pairings
  outcomes  Inspect first-party outcomes, linkage, and sale intervals
  report    Read Analysis A and the accumulation status of Analyses B and C

Options:
      --json  one JSON document using API field names
  -h, --help  Print help
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
Reachability/freshness of AHledger, roster, sales log, catalogue and vendors

Usage: everledger status [OPTIONS]

Options:
      --json  one JSON document using API field names
  -h, --help  Print help
```

## refresh

```text
Rebuild catalogue and refresh the configured vendor backend

Usage: everledger refresh [OPTIONS] <TARGET>

Arguments:
  <TARGET>  [possible values: catalogue]

Options:
      --json  one JSON document using API field names
  -h, --help  Print help
```

