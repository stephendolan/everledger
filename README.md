# everledger

A command-line companion for **WoW: Forever** (Alliance, PvP ruleset, US). One tool answers the questions that come up while playing:

- **Recipes**: learn levels, skill-up bands (orange/yellow/green/grey), reagents, and where a recipe comes from.
- **Auction house prices**: current medians, floor prices, listing counts, and sales volume, plus per-craft margins priced from live scans.
- **Character state**: level, gold, hearth, profession ranks, known recipes, and current auction postings, read from a self-hosted companion roster service.
- **Vendor lookup** (in development): which NPC sells an item, where, and at what vendor price.

The tool keeps its sources separate on purpose. Recipe facts come only from the local catalogue. Prices come only from AHledger. Character state comes only from the roster service. A margin is quoted at the floor price when the shelf is deep, not at the median, because a new listing earns the floor.

## Install

Requirements: Python 3.11+ and `curl`. No third-party packages.

```bash
git clone <this repo>
cd everledger
chmod +x everledger
./everledger refresh   # builds the local recipe catalogue (see below)
./everledger verify    # checks the catalogue against known anchor recipes
```

Put `everledger` on your `PATH`, or call it by path.

## Data

Recipe data is not committed to this repo. `everledger refresh` downloads the profession index pages from wowforever.ninja into a local data directory and builds `catalogue.json` from them (about 2,200 recipes across 12 professions). Re-run it whenever the game patches. The catalogue warns when it is more than 14 days old.

Prices are fetched live from the AHledger API on each `price` or `margins` call. Nothing is cached.

Character state needs the companion roster service, which reads the game's addon SavedVariables (Syndicator, DataStore) and serves them over HTTP. That service is not part of this repo. Without it, `char` fails and `recipe` simply skips its known-by annotation; everything else works.

## Configuration

All configuration is by environment variable. See `.env.example` for the full list.

| Variable | Default | Purpose |
|---|---|---|
| `EVERLEDGER_DATA_DIR` | `~/.everledger` | Where `catalogue.json` and the raw page cache live |
| `EVERLEDGER_ROSTER_URL` | `http://localhost:8765` | Base URL of the companion roster service |
| `EVERLEDGER_ROSTER_TOKEN` | (none) | Bearer token for the roster service |
| `EVERLEDGER_ROSTER_TOKEN_FILE` | (none) | Path to a file holding the token, if not set directly |
| `EVERLEDGER_MARKET` | `forever.pvp.alliance.us` | AHledger market to price against |

The default market is the author's: `forever.pvp.alliance.us` (Alliance, PvP, US). Set `EVERLEDGER_MARKET` to price against a different market.

## Usage

```bash
# Recipe lookup, optionally coloured at a given skill level
everledger recipe "Phoenix Pants"
everledger recipe --skill 117 "Bolt of Silk Cloth"

# Which recipes give skill points across a range (a levelling bridge)
everledger bridge tailoring 100 125

# Craft margins at a skill level, priced from the latest AHledger scan
everledger margins tailoring 117
everledger margins --known <character> tailoring 117

# Character state, professions, skill-up options, current auctions
everledger char <character>
everledger char <character> tailoring
everledger char <character> auctions

# Item price: median, floor, listed count, 7-day median, estimated sales/day
everledger price "Bolt of Silk Cloth"
everledger price 2604

# Catalogue health check
everledger verify
```

Sample output (`price`):

```
Red Dye (item 2604) latest 2026-11-14T18:49Z: median 88c, min 87c, listed 709 in 13 auctions | 7d median 92c | est 380.8/day over 79.7h coverage. Prices from AHledger.
```

## How it works

- `build_catalogue.py` parses the wowforever.ninja profession pages into `catalogue.json`. It asserts a set of anchor recipes whose learn levels and sources are verified, because marketplace metadata for those fields is unreliable.
- `refresh.sh` fetches the pages and runs the builder. `everledger refresh` calls it.
- `everledger` itself is a single Python file using only the standard library.

## Credits

- **Prices from AHledger** (https://ahledger.com). All auction house prices, listing counts, and sales estimates come from AHledger's API for the `forever.pvp.alliance.us` market.
- **Recipe data from wowforever.ninja** (https://wowforever.ninja), which publishes WoW: Forever client data. Its vendor and world data derive in part from the cMaNGOS classic-db baseline and are GPL-3.0 with attribution; the catalogue built by this tool inherits that. Treat baseline rows as unconfirmed until checked against the live game: Forever moves vendors and sources.
- **Data from [ForeverDB.net](https://foreverdb.net)**. Vendor data (which NPC sells what, where, and at what price) comes from ForeverDB, extracted from the Forever client. The vendor lookup now in development is built on it.

World of Warcraft and WoW: Forever are not affiliated with this project. This is a personal, non-commercial tool.
