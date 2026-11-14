# everledger

The command reference below is generated from `--help`.

```text
usage: - [-h]
         {price,recipe,bridge,margins,vendors,char,listings,positions,sales,purchases,expirations,net,events,status,refresh,verify} ...

everledger: WoW Forever prices, catalogue, vendors, characters and outcomes.

positional arguments:
  {price,recipe,bridge,margins,vendors,char,listings,positions,sales,purchases,expirations,net,events,status,refresh,verify}
    price               AHledger quote plus vendor listed prices
    recipe              Catalogue facts and roster known-by annotation
    bridge              Catalogue skill-up bands across a range
    margins             AHledger craft margins; optional roster recipe/level
                        scope
    vendors             Vendor listed prices/locations; faction unavailable
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
    refresh             Rebuild catalogue and refresh previously queried
                        vendor items
    verify              Deprecated catalogue anchor check (use status)

options:
  -h, --help            show this help message and exit

Selectors: --char accepts a unique first or full character name.
Times: 7d, 24h, dates, local ISO timestamps, or UTC ISO with Z; since inclusive,
until exclusive. Outcomes default to 7d; events to 24h. Limits: 50 human, 500 JSON.
JSON uses API field names, integer copper totals and UTC ISO timestamps.
Human output ends with source names, save/publication times and ages.
Exit codes: 0 answer, 1 error, 2 usage, 3 stale data printed, 4 source unreachable.
Journal save age is normal: outcomes and net exit 0 even for old saves.
Price/margins AH freshness limit: 15m; catalogue advisory age: 14d.
Listings use the roster scan limit (15m). Positions apply both market/scan limits.
Vendor queries cache snapshots until refresh catalogue; after 1d they are stale.
On refresh failure a cache up to 7d old prints stale with exit 3, else exit 4.
Vendor listed prices and sale claims are Classic 1.12 baseline. Vendor sightings
alone may be seen in Forever. Purchase quantity and faction are unavailable;
all zones appear, including Horde. Listed prices are not used in craft margins.
Configuration: Python 3.11+ and curl; keep this executable beside everledger_*.py.
EVERLEDGER_DATA_DIR defaults to ~/.everledger (catalogue and queried vendor cache).
EVERLEDGER_ROSTER_URL defaults to http://localhost:8765; set EVERLEDGER_ROSTER_TOKEN
or EVERLEDGER_ROSTER_TOKEN_FILE for bearer authentication.
EVERLEDGER_MARKET defaults to forever.pvp.alliance.us.
EVERLEDGER_AHLEDGER_URL defaults to https://api.ahledger.com.
EVERLEDGER_VENDOR_URL defaults to https://wowforever.ninja.
Refresh downloads profession pages and refreshes only previously queried vendor IDs.
Deprecated for one release: verify (use status), bare refresh (use refresh catalogue),
margins --known NAME (use --char NAME). Warnings go to stderr; JSON stays stable.
char NAME auctions prints listings --char NAME on stderr and exits 2.
```

## price

```text
usage: - price [-h] [--json] item [item ...]

AHledger quote plus vendor listed prices

positional arguments:
  item

options:
  -h, --help  show this help message and exit
  --json      one JSON document using API field names
```

## recipe

```text
usage: - recipe [-h] [--skill SKILL] [--json] item [item ...]

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
usage: - bridge [-h] [--json] profession start end

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
usage: - margins [-h] [--level LEVEL] [--char CHAR] [--json] [profession]

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
usage: - vendors [-h] [--in ZONE] [--char CHAR] [--all] [--json]
                 item [item ...]

Vendor listed prices/locations; faction unavailable

positional arguments:
  item

options:
  -h, --help   show this help message and exit
  --in ZONE    zone/city substring; aliases IF, SW, Darn, Org, TB, UC
  --char CHAR  character scope; faction filtering unavailable
  --all        all zones (also the default: faction unavailable)
  --json       one JSON document using API field names
```

## char

```text
usage: - char [-h] [--skill SKILL] [--json] [name] [profession]

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
usage: - listings [-h] [--char CHAR] [--item ITEM] [--json]

Recorded live auction listings and AHledger floor comparison

options:
  -h, --help   show this help message and exit
  --char CHAR
  --item ITEM
  --json       one JSON document using API field names
```

## positions

```text
usage: - positions [-h] [--char CHAR] [--item ITEM] [--json]

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
usage: - sales [-h] [--item ITEM] [--char CHAR] [--since SINCE]
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
usage: - purchases [-h] [--item ITEM] [--char CHAR] [--since SINCE]
                   [--until UNTIL] [--venue {auction,vendor}] [--json]

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
usage: - expirations [-h] [--item ITEM] [--char CHAR] [--since SINCE]
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
usage: - net [-h] [--char CHAR] [--since SINCE] [--until UNTIL] [--json]
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
usage: - events [-h] [--item ITEM] [--char CHAR] [--since SINCE]
                [--until UNTIL] [--venue {auction,vendor}] [--type TYPE]
                [--cursor CURSOR] [--after AFTER] [--limit LIMIT] [--json]

Typed journal feed; prints next_cursor, has_more and service checkpoint

options:
  -h, --help            show this help message and exit
  --item ITEM
  --char CHAR
  --since SINCE
  --until UNTIL
  --venue {auction,vendor}
  --type TYPE           comma-separated domain types
  --cursor CURSOR       snapshot page cursor from next_cursor; inherits fixed
                        filters
  --after AFTER         opaque cursor/checkpoint; exclusive with since/until
  --limit LIMIT         0 bootstraps a watch; default 50 human, 500 JSON
  --json                one JSON document using API field names

Start a watch: everledger events --limit 0 --json. Save the service checkpoint
for after when supplied; next_cursor finishes pagination via the service
cursor parameter. The feed never skips or repeats across cursors; a worker
that acts before saving next_cursor delivers at least once. Filters may change
between polls. On 409 cursor_invalid, alert once and resync from resync_since;
this command retries once. snapshot_changed requires restarting from the last
committed checkpoint. This CLI persists nothing.
```

## status

```text
usage: - status [-h] [--json]

Reachability/freshness of AHledger, roster, outcomes, catalogue and vendors

options:
  -h, --help  show this help message and exit
  --json      one JSON document using API field names
```

## refresh

```text
usage: - refresh [-h] [--json] [{catalogue}]

Rebuild catalogue and refresh previously queried vendor items

positional arguments:
  {catalogue}

options:
  -h, --help   show this help message and exit
  --json       one JSON document using API field names
```

## verify

```text
usage: - verify [-h] [--json]

Deprecated catalogue anchor check (use status)

options:
  -h, --help  show this help message and exit
  --json      one JSON document using API field names
```

## Credits

- **Prices from AHledger** (https://ahledger.com). All auction house prices, listing counts, and sales estimates come from AHledger's API for the `forever.pvp.alliance.us` market.
- **Recipe data from wowforever.ninja** (https://wowforever.ninja), which publishes WoW: Forever client data. Its vendor and world data derive in part from the cMaNGOS classic-db baseline and are GPL-3.0 with attribution; the catalogue built by this tool inherits that. Treat baseline rows as unconfirmed until checked against the live game: Forever moves vendors and sources.
- **Data from [ForeverDB.net](https://foreverdb.net)**. Vendor data (which NPC sells what, where, and at what price) comes from ForeverDB, extracted from the Forever client. The vendor lookup now in development is built on it.

World of Warcraft and WoW: Forever are not affiliated with this project. This is a personal, non-commercial tool.
