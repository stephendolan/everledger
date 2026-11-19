# everledger

everledger is a command-line tool for WoW: Forever prices, crafting, vendors,
characters, and journal outcomes, built as one Rust binary. It reads auction
prices and sales estimates from AHledger, recipes and baseline vendor data from
wowforever.ninja, client vendor data from ForeverDB, and characters, inventory,
listings, and journal records from a companion roster service.

## Build

[Rust 1.100.0](rust-toolchain.toml) is pinned for this project. Build the release
binary from the repository root:

```sh
cargo build --release
```

The deployed host takes the static binary at
`target/x86_64-unknown-linux-musl/release/everledger`:

```sh
rustup target add x86_64-unknown-linux-musl
CC_x86_64_unknown_linux_musl=gcc cargo build --release --target x86_64-unknown-linux-musl
```

`CC_x86_64_unknown_linux_musl` names the C compiler for the TLS library's C
sources; `gcc` serves where no musl compiler wrapper is installed. The binary
verifies TLS against the host's certificate bundle in the standard locations
or the file named by `SSL_CERT_FILE`. Connector transport runs `python3` from
`PATH` and needs the `dynamic_credentials` helper.

## Run

Build the local recipe catalogue on the first run:

```sh
target/release/everledger refresh catalogue
```

Use `target/release/everledger` followed by a command. See the
[command reference](docs/command-reference.md) for arguments and options.

| Command | Answers |
| --- | --- |
| `price` | AHledger quote and listed vendor prices for an item. |
| `recipe` | Recipe facts and which roster characters know it. |
| `bridge` | Profession skill-up bands across a range. |
| `margins` | Craft margins, optionally scoped to a character or level. |
| `vendors` | Listed prices and seller locations, with faction from ForeverDB. |
| `char` | Characters, profession ranks, and known recipes. |
| `listings` | Recorded auctions compared with the AHledger floor. |
| `positions` | Holdings and listings valued at the floor; cost basis needs assignments. |
| `sales` | Sales proceeds, with auction sales as the default. |
| `purchases` | Auction and vendor purchases. |
| `expirations` | Expired auctions and market inputs for reposting. |
| `net` | Item auction cash and counts; profit basis remains unknown. |
| `events` | Paged journal events and service checkpoints for consumers. |
| `status` | Source reachability, cache freshness, and vendor backend state. |
| `refresh catalogue` | Rebuild the catalogue and refresh the configured vendor backend. |

Every command accepts `--json` for one JSON document with API field names,
integer copper amounts, and UTC timestamps. Human output labels unknown values
and ends with a source summary. Exit codes are:

| Code | Meaning |
| --- | --- |
| `0` | Answer returned. |
| `1` | Error. |
| `2` | Usage error. |
| `3` | Stale data printed. |
| `4` | Source unreachable. |

## Environment

Set variables in the process environment. [.env.example](.env.example) lists
the same settings; the binary does not load that file itself.

| Variable | Default | Purpose |
| --- | --- | --- |
| `HOME` | `.` when absent | Supplies the home directory for the default data path. |
| `EVERLEDGER_DATA_DIR` | `$HOME/.everledger` | Catalogue, downloaded pages, and vendor caches. |
| `EVERLEDGER_ROSTER_URL` | `http://localhost:8765` | Companion roster service base URL. |
| `EVERLEDGER_ROSTER_TOKEN` | Unset | Bearer token; a nonblank value takes precedence over the token file. |
| `EVERLEDGER_ROSTER_TOKEN_FILE` | Unset | File containing the bearer token. |
| `EVERLEDGER_MARKET` | `forever.pvp.alliance.us` | AHledger market. |
| `EVERLEDGER_AHLEDGER_URL` | `https://api.ahledger.com` | AHledger base URL override. |
| `EVERLEDGER_VENDOR_URL` | `https://wowforever.ninja` | Recipe and ninja vendor source URL override. |
| `EVERLEDGER_FOREVERDB_API_KEY` | Unset | ForeverDB key; a nonblank value takes precedence over the key file. |
| `EVERLEDGER_FOREVERDB_API_KEY_FILE` | `$EVERLEDGER_DATA_DIR/foreverdb-api-key` | File containing the ForeverDB key. |
| `EVERLEDGER_FOREVERDB_VIA_CONNECTOR` | Unset | Nonblank connector slug, such as `custom.foreverdb`; takes precedence over key settings. |
| `EVERLEDGER_DYNAMIC_CREDENTIALS_DIR` | `/opt/muse/skills/custom-connector-setup/bin` | Directory containing the connector's `dynamic_credentials` helper. |
| `EVERLEDGER_FOREVERDB_DAILY_REQUESTS` | `36` | Positive integer request budget over a rolling 24 hours, shared across refresh attempts and retries. |

One refresh can spend only the budget remaining when its attempt starts.
Spends become available as they age out of the rolling 24-hour window; repeating
a refresh does not reset the allowance. An invalid budget value fails every
command with exit code 2, including help. See the [ForeverDB reference](docs/foreverdb.md)
for request sizing, activation, and resumption.

HTTP proxy settings and `SSL_CERT_FILE` are honored. Test clock settings belong
to the [parity seam contract](parity/README.md#seam-contract).

## Python fallback

The [Python fallback](python/README.md) lives in `python/` and needs Python 3.11+
and curl. From the repository root:

```sh
python/everledger --help
```

It reads the same environment and data directory. The binary and fallback can
take turns on one data directory, including resuming each other's vendor syncs.
A wrapper script must point at `python/everledger` in this checkout.

## Verification

```sh
cargo test
python3 scripts/update_command_reference.py target/release/everledger
```

The tests compare every generated reference block with the binary's help. The
[parity harness](parity/README.md) compares the Rust binary with the Python
reference using loopback fixtures, including requests, output, and cache bytes.
Its matrix runner runs every instrument. Maintainers can also read the
[architecture](docs/architecture.md), [sanctioned differences](docs/rust-port-differences.md),
and [journal consumer contract](docs/journalator-contract.md).

## Credits

- **Prices from AHledger** (https://ahledger.com). All auction house prices, listing counts, and sales estimates come from AHledger's API for the `forever.pvp.alliance.us` market.
- **Recipe data from wowforever.ninja** (https://wowforever.ninja), which publishes WoW: Forever client data. Its vendor and world data derive in part from the cMaNGOS classic-db baseline and are GPL-3.0 with attribution; the catalogue built by this tool inherits that. Treat baseline rows as unconfirmed until checked against the live game: Forever moves vendors and sources.
- **Data from [ForeverDB.net](https://foreverdb.net)**. The optional ForeverDB adapter indexes vendor data extracted from the Forever client. It activates after a key or connector is configured and a complete sync finishes.

World of Warcraft and WoW: Forever are not affiliated with this project. This is a personal, non-commercial tool.
