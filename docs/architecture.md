# Architecture of the Rust crate

`everledger` is one crate: a library and a thin executable. Rust 1.100.0 is
pinned in `rust-toolchain.toml`, and every dependency is pinned to an exact
version. The crate forbids `unsafe`.

This file records the conventions the code cannot state for itself. For what a
function takes and returns, read the code.

## Layout

| Path | Holds |
| --- | --- |
| `src/lib.rs`, `src/main.rs` | Library exports, collaborators, process setup, and exit. |
| `src/cli.rs`, `src/cli_help.txt` | Command table, parser rules, dispatch, and help epilogue. |
| `src/commands/`, `src/human/` | Command logic and human rendering; each command has a file in each directory. |
| `src/roster.rs`, `src/read_roster.rs`, `src/journal.rs` | Roster transport, character views, journal pages, aggregates, and cursors. |
| `src/market.rs` | AHledger price tables and quantity decoding. |
| `src/vendors.rs`, `src/vendors/foreverdb_answer.rs`, `src/vendors/status.rs` | Backend selection, ninja data, ForeverDB answers, coverage, and status. |
| `src/foreverdb.rs`, `src/foreverdb/` | Sync engine, mapping, transports, interrupt handling, and `src/foreverdb/connector.py`. |
| `src/catalogue.rs`, `src/catalogue_build.rs`, `src/catalogue_check.rs` | Catalogue views, profession-page parsing, and anchor checks. |
| `src/document.rs`, `src/document/decode.rs`, `src/document/tests.rs` | Ordered documents, typed decoding, and document unit tests. |
| `src/http.rs`, `src/http/forward.rs` | HTTP requests, payloads, and typed forwarding from documents. |
| `src/types.rs`, `src/primitives.rs`, `src/time.rs` | Units, failures, and Python-compatible scalar and time operations. |
| `src/config.rs`, `src/clock.rs`, `src/provenance.rs`, `src/diagnostics.rs` | Configuration, clocks, source records, and the stderr sink. |
| `src/generated/html_entities.json`, `src/unicode.tsv` | HTML entity and Unicode tables. |
| `tests/` | Integration tests, golden vectors, the oracle probe, and shared fixtures. |
| `python/` | Frozen reference implementation, fallback, and its tests. |
| `parity/` | Differential harness, instruments, matrix, and build comparison. |

A module with submodules keeps its parent file and places the submodules in a
directory with the same name: `src/document.rs` and `src/document/`, for example.
The same rule applies to HTTP, vendors, and ForeverDB.

## Conventions

**Output is written by `document::dumps`, never by serde_json.** The contract is
byte parity with the Python reference, whose output is defined by `json.dumps`,
float `repr`, and dict insertion order. `document.rs` and `primitives.rs`
reproduce those, and `tests/oracle_probe.rs` holds them to the reference.

**A payload is a document and a view.** `Payload<T>` keeps the ordered document
that arrived and a typed view decoded from it. Logic reads the view; output
re-emits the document, so fields the CLI never looked at pass through in their
original order. `set` and `set_default` change the document in place and decode
the view again; an update that does not decode is undone, so the two cannot
disagree. `Field<T>` (`Missing`, `Null`, `Value`) is
for the places where the contract tells an absent key from a null one.

**A view is exactly as strict as the contract.** A required field is a required
serde field and an optional one is an `Option`. A payload that does not fit
fails where it is decoded, with a `Failure` naming the boundary.

**Units are types.** `Copper`, `EpochSeconds`, and `UtcInstant` (valid from year
1 through year 9999) carry money and time everywhere past the boundary.

**Fixed sets of cases are enums, matched without a wildcard.** The journal query
(`Window`, `Watch`, `Page`), the end of a page (`More`, `Complete`), vendor
coverage (`ItemAbsent`, `PinsAbsent`, `Sellers`), and the vendor answer
(`Ninja`, `Foreverdb`) are the main ones. A watch checkpoint, a service page
cursor, and an opaque page cursor are three different types.

**Human strings come from `src/human/` only.** A command returns a
`CommandOutput`: the document, the human lines, whether the answer is stale,
and an exit code for what staleness does not decide. `human::emit` writes the
document, or the lines followed by the stale notice and the single `source:`
footer. `CommandOutput::exit_code` returns 3 for a stale answer and otherwise
returns the command's own code.

**One decision gives the stale notice and exit code 3.** Each command decides
from the sources it holds to a freshness limit whether its answer is stale.
That flag prints the notice and makes the exit code 3; nothing is inferred
from the sources afterwards.

**Warnings are written when they happen.** A command writes a deprecation,
resync notice, or cache notice through `context.diagnostics`, so the line
survives a failure that follows it.

**One command table drives the CLI.** The `commands!` table in `src/cli.rs`
defines the command enum, selects the shared `--json` flag through `OutputArgs`,
and generates exhaustive dispatch.

**Journal rows have one reading.** `Event::item` supplies a row's item;
`Journal::rows` supplies a service page's rows. Outcome commands build a distinct
`Aggregate` with its own `events` array and `rows` accessor, rather than reading
it as a service page.

**Collaborators are explicit.** `Context` carries the clock, HTTP, configuration,
catalogue store, and diagnostics, so command logic can run in-process against
fixtures.

**Credentials cannot be printed.** `config::Secret` and the transport's `ApiKey`
have no formatting that shows their value.

**No path that depends on input data can panic.** Failures carry one of the
five exit codes.

## Vendor backend state

`BackendState` in `src/vendors.rs` loads the selected transport and configured
index once for the command. It reads the attempt journal lazily and retains
the result, so a command opens only the files its answer needs. With no
transport configured, it opens neither index nor journal. A lookup without a
completed index can fall back to ninja without opening the journal; status
reads it when reporting the last attempt. The same state supplies backend
selection, stale eligibility, and failure status.

## The ForeverDB sync

A sync spends requests over a rolling 24 hours. The [ForeverDB reference](foreverdb.md#requests-and-the-rolling-budget)
owns the cap, request arithmetic, and examples. `plan_sync` reads the index,
journal, and checkpoint and fixes the attempt's remaining allowance before
opening it. Correctness depends on this ordering:

- The attempt journal is written before each request is sent, so a request is
  counted even if the process dies while it is in flight. The next reader
  dates that attempt's spend from its last request start.
- `walk_pages` and `walk_details` write the checkpoint after each finished
  page or detail.
- The index is published by atomic replace only when every detail is present.
- The budget is fixed when an attempt starts. Spends that expire while it runs
  leave the journal and add nothing to it.
- Within an attempt, request starts are at least 2.1 seconds apart at any cap.
- An interrupt during a sync closes the journal on a second thread and then
  ends the process as SIGINT ends one, whatever fails on the way, so a stopped
  process never keeps the lock. That thread holds the snapshot's lock until
  the process is gone, so the syncing thread can neither write nor send after
  it. Outside a sync SIGINT has its default effect.
- One `flock` on `sync.lock` serializes syncs, including against the Python
  implementation on the same data directory.

`load_attempt` adds the spend of an unfinished attempt and drops expired
spends when it reads `attempt.json` for a sync. Every journal write drops them
again. `status` and `vendors` read the file plainly:
they use its `failed` flag and the last attempt's time and request count, and
nothing about spends.

`reserve_request` reserves a request before counting it. The journal is written
with the next count first, and the counters in memory move only when that write
succeeds, so a failed write neither counts a request nor sends one.

`close_attempt` records completion, pending work, or failure. The sync result
contains `configured`, `complete`, `pending`, `requests`, `total_requests`,
`failed`, `deferred`, `status`, and `error`. A failed request, mapping, or write is part of that result, and
`refresh` reports it on stdout with exit code 4. Two things leave by another
route: a cache file that cannot be decoded, which is an error, and an
interrupt.

A retryable 502 is retried once. The attempt waits for it only when the wait
is 300 seconds or less. A longer wait is recorded in the checkpoint as
`not_before`, the attempt ends pending, and a later attempt takes it up.

A cache file is read as the reference reads it. JSON `null` is no file. An
empty container is no index and no journal to a lookup, and an unusable
checkpoint to a sync. Anything else that is not an object is an error. Errors
keep the reference's three kinds (`CacheError`): a file that cannot be decoded
ends the command with exit code 1; a failure or an I/O error inside a sync
becomes a failed result; and after a failed sync `refresh` skips only the
kinds the reference skips.

The cap is read once, before the arguments, into `config::DailyRequestCap`,
which cannot be zero. A value that is not a positive integer ends every
invocation as a usage error, whatever the command.

The connector transport runs each request through `src/foreverdb/connector.py`, a
Python process that calls the `dynamic_credentials` helper and performs the
request itself. The helper may replace the request rather than add a header to
it, and the surrogate credential never enters the Rust process.

## Public-source transport

`Request::public_get` marks Rust's AHledger quote and pricetable, ninja item and
pins, vendor status probe, and profession-page requests. Python marks the same
requests with `public=True`; `refresh.sh` downloads each profession page through
`everledger_fetch.py` and the shared core request function.
`EVERLEDGER_FETCH_TRANSPORT` selects native, curl, or the default auto fallback.
Only native transport errors trigger the single curl attempt. HTTP statuses and
local request errors do not. Both transports feed the same status/body handling;
failed or absent curl retains the original native error. Roster and ForeverDB
requests do not opt in. AHledger item-name resolution currently uses the local
catalogue or quote response; there is no separate `/v1/items` fetch.

## Verification

`cargo test` runs unit and integration tests, golden vectors, the oracle probe,
and the generated [command reference](command-reference.md) check. The
[parity guide](../parity/README.md) gives the commands for:

- `parity/matrix.py`, which runs every differential instrument against the
  Python reference, including captured roster shapes and sync recovery.
- `parity/compare_builds.py`, which compares two seam builds' candidate
  transcripts without the matrix's policy exemptions.

The [difference register](rust-port-differences.md) names each sanctioned
difference and its instrument. The `parity-seams` feature supplies a virtual
clock, a sleep log, and a loopback ForeverDB address. Builds without that
feature contain none of these overrides. `.cargo/config.toml` sets a closed loopback proxy
and loopback bypass for programs launched by Cargo. Invoking the built binary
directly uses the process environment.
