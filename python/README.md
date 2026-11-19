# Python reference and fallback

This directory holds the Python reference for verifying the Rust binary and
the fallback for hosts without it. It requires Python 3.11+ and curl.

Run it from the repository root:

```sh
python/everledger --help
```

The executable also runs from any working directory when called by its path.
A wrapper must point at `python/everledger`. It reads the same
[environment settings](../README.md#environment) and data directory as the
binary. `EVERLEDGER_DATA_DIR` defaults to `~/.everledger`; the two implementations
can take turns reading and writing it.

Run the reference tests from the repository root:

```sh
PYTHONPATH=python python3 -m unittest discover -s python/tests -v
PYTHONPATH=python /usr/bin/python3.12 -m unittest discover -s python/tests -v
```

The command reference below is checked against the executable by
`tests/test_cli.py`.

```text
usage: everledger [-h]
                  {price,recipe,bridge,margins,vendors,char,listings,positions,sales,purchases,expirations,net,events,status,refresh} ...

everledger: WoW Forever prices, catalogue, vendors, characters and outcomes.

positional arguments:
  {price,recipe,bridge,margins,vendors,char,listings,positions,sales,purchases,expirations,net,events,status,refresh}
    price               AHledger quote plus vendor listed prices
    recipe              Catalogue facts and roster known-by annotation
    bridge              Catalogue skill-up bands across a range
    margins             AHledger craft margins; optional roster recipe/level
                        scope
    vendors             Vendor listed prices/locations; ForeverDB includes
                        faction
    char                Characters, profession ranks and known recipes
    listings            Recorded live auction listings and AHledger floor
                        comparison
    positions           Inventory holdings/listings and AHledger floor; basis
                        unknown without assignments
    sales               Sales proceeds (defaults to auction)
    purchases           Auction and vendor purchases
    expirations         Expired auctions with market repost inputs
    net                 Item auction cash/count diagnostic; profit basis stays
                        unknown
    events              Typed journal feed; prints next_cursor, has_more and
                        service checkpoint
    status              Reachability/freshness of AHledger, roster, outcomes,
                        catalogue and vendors
    refresh             Rebuild catalogue and refresh the configured vendor
                        backend

options:
  -h, --help            show this help message and exit

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
Price/margins AH freshness limit: 15m; catalogue advisory age: 14d.
Listings use the roster scan limit (15m). Positions apply both market/scan limits.
Vendor queries cache snapshots until refresh catalogue; after 1d they are stale.
On refresh failure a cache up to 7d old prints stale with exit 3, else exit 4.
Ninja sale/price claims are Classic 1.12 baseline; vendor sightings may be seen
in Forever. ForeverDB maps its own sell-row labels separately from sightings.
Ninja quantity/faction are unavailable; ForeverDB retains stack and side.
All zones appear, including Horde; faction filtering is unavailable. Listed prices are not used in craft margins.
Configuration: Python 3.11+ and curl; keep this executable beside everledger_*.py.
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
usage: everledger price [-h] [--json] item [item ...]

AHledger quote plus vendor listed prices

positional arguments:
  item

options:
  -h, --help  show this help message and exit
  --json      one JSON document using API field names
```

## recipe

```text
usage: everledger recipe [-h] [--skill SKILL] [--json] item [item ...]

Catalogue facts and roster known-by annotation

positional arguments:
  item

options:
  -h, --help     show this help message and exit
  --skill SKILL
  --json         one JSON document using API field names
```

## bridge

```text
usage: everledger bridge [-h] [--json] profession start end

Catalogue skill-up bands across a range

positional arguments:
  profession
  start
  end

options:
  -h, --help  show this help message and exit
  --json      one JSON document using API field names
```

## margins

```text
usage: everledger margins [-h] [--level LEVEL] [--char CHAR] [--json]
                          [profession]

AHledger craft margins; optional roster recipe/level scope

positional arguments:
  profession

options:
  -h, --help     show this help message and exit
  --level LEVEL
  --char CHAR
  --json         one JSON document using API field names
```

## vendors

```text
usage: everledger vendors [-h] [--in ZONE] [--json] item [item ...]

Vendor listed prices/locations; ForeverDB includes faction

positional arguments:
  item

options:
  -h, --help  show this help message and exit
  --in ZONE   zone/city substring; aliases IF, SW, Darn, Org, TB, UC
  --json      one JSON document using API field names
```

## char

```text
usage: everledger char [-h] [--skill SKILL] [--json] [name] [profession]

Characters, profession ranks and known recipes

positional arguments:
  name
  profession

options:
  -h, --help     show this help message and exit
  --skill SKILL
  --json         one JSON document using API field names
```

## listings

```text
usage: everledger listings [-h] [--char CHAR] [--item ITEM] [--json]

Recorded live auction listings and AHledger floor comparison

options:
  -h, --help   show this help message and exit
  --char CHAR
  --item ITEM
  --json       one JSON document using API field names
```

## positions

```text
usage: everledger positions [-h] [--char CHAR] [--item ITEM] [--json]

Inventory holdings/listings and AHledger floor; basis unknown without
assignments

options:
  -h, --help   show this help message and exit
  --char CHAR
  --item ITEM
  --json       one JSON document using API field names
```

## sales

```text
usage: everledger sales [-h] [--item ITEM] [--char CHAR] [--since SINCE]
                        [--until UNTIL] [--venue {auction,vendor}] [--json]

Sales proceeds (defaults to auction)

options:
  -h, --help            show this help message and exit
  --item ITEM
  --char CHAR
  --since SINCE
  --until UNTIL
  --venue {auction,vendor}
  --json                one JSON document using API field names
```

## purchases

```text
usage: everledger purchases [-h] [--item ITEM] [--char CHAR] [--since SINCE]
                            [--until UNTIL] [--venue {auction,vendor}]
                            [--json]

Auction and vendor purchases

options:
  -h, --help            show this help message and exit
  --item ITEM
  --char CHAR
  --since SINCE
  --until UNTIL
  --venue {auction,vendor}
  --json                one JSON document using API field names
```

## expirations

```text
usage: everledger expirations [-h] [--item ITEM] [--char CHAR] [--since SINCE]
                              [--until UNTIL] [--json]

Expired auctions with market repost inputs

options:
  -h, --help     show this help message and exit
  --item ITEM
  --char CHAR
  --since SINCE
  --until UNTIL
  --json         one JSON document using API field names
```

## net

```text
usage: everledger net [-h] [--char CHAR] [--since SINCE] [--until UNTIL]
                      [--json]
                      item [item ...]

Item auction cash/count diagnostic; profit basis stays unknown

positional arguments:
  item

options:
  -h, --help     show this help message and exit
  --char CHAR
  --since SINCE
  --until UNTIL
  --json         one JSON document using API field names
```

## events

```text
usage: everledger events [-h] [--item ITEM] [--char CHAR] [--since SINCE]
                         [--until UNTIL] [--venue {auction,vendor}]
                         [--type TYPE] [--cursor CURSOR] [--after AFTER]
                         [--limit LIMIT] [--json]

Typed journal feed; prints next_cursor, has_more and service checkpoint

options:
  -h, --help            show this help message and exit
  --item ITEM
  --char CHAR
  --since SINCE
  --until UNTIL
  --venue {auction,vendor}
  --type TYPE           comma-separated purchase, sale, posting, expiration,
                        cancellation
  --cursor CURSOR       snapshot page cursor from next_cursor; inherits fixed
                        filters
  --after AFTER         terminal after_cursor/checkpoint; exclusive with
                        since/until
  --limit LIMIT         0 bootstraps a watch; default 50 human, 500 JSON
  --json                one JSON document using API field names

Start a watch: everledger events --limit 0 --json. Bootstrap drains a snapshot
with positive service limits and prints no entries. Save after_cursor (the
service terminal checkpoint) only after has_more is false. While has_more is
true, pass next_cursor to --cursor to finish the snapshot; page cursors retain
the local view. Character/item/type/venue filters are local views over an
unfiltered checkpoint and may change between polls. Limit bounds the scanned
page, so a filtered page may be empty with has_more true. Times are sent as
integer epoch seconds. On 409 cursor_invalid, alert once and resync from
resync_since; retries can replay entries. snapshot_changed requires restarting
from the last committed checkpoint. Act before saving progress for at-least-
once delivery. This CLI persists nothing.
```

## status

```text
usage: everledger status [-h] [--json]

Reachability/freshness of AHledger, roster, outcomes, catalogue and vendors

options:
  -h, --help  show this help message and exit
  --json      one JSON document using API field names
```

## refresh

```text
usage: everledger refresh [-h] [--json] {catalogue}

Rebuild catalogue and refresh the configured vendor backend

positional arguments:
  {catalogue}

options:
  -h, --help   show this help message and exit
  --json       one JSON document using API field names
```

