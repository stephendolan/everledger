# Architecture of the Rust crate

`everledger` is one crate: a library and a thin executable. Rust 1.100.0 is
pinned in `rust-toolchain.toml`, and every dependency is pinned to an exact
version. The crate forbids `unsafe`.

This file records the conventions the code cannot state for itself. For what a
function takes and returns, read the code.

## Layout

| Path | Holds |
| --- | --- |
| `src/main.rs` | Parses arguments, builds the collaborators, runs, exits with the code. |
| `src/cli.rs` | The `Command` enum, exhaustive dispatch, and the argument rules argparse applies that clap does not. |
| `src/commands/<name>.rs` | One command: its `Args` struct and its logic. |
| `src/human/<name>.rs` | That command's human rendering. |
| `src/roster.rs`, `read_roster.rs`, `journal.rs` | The roster service: transport, character views, and the event stream with its cursor states. |
| `src/market.rs` | The AHledger price table. |
| `src/vendors.rs`, `vendors_foreverdb.rs`, `vendor_status.rs` | Vendor answers from either backend, backend selection, and backend status. |
| `src/foreverdb.rs`, `foreverdb_mapping.rs`, `foreverdb_transport.rs`, `foreverdb_interrupt.rs`, `foreverdb_connector.py` | The ForeverDB sync, its strict mapping, both transports, interrupt handling, and the connector shim. |
| `src/catalogue.rs`, `catalogue_build.rs` | The catalogue file and the build that produces it from profession pages. |
| `src/document.rs`, `document_decode.rs`, `primitives.rs`, `time.rs` | The ordered document with its Python-compatible writer, typed decoding straight from a document, and text, number, and time primitives. |
| `src/types.rs`, `config.rs`, `clock.rs`, `http.rs`, `provenance.rs`, `diagnostics.rs` | Units and failures, configuration, clocks, HTTP, sources, and the stderr sink. |

## Conventions

**Output is written by `document::dumps`, never by serde_json.** The contract is
byte parity with the Python reference, whose output is defined by `json.dumps`,
float `repr`, and dict insertion order. `document.rs` and `primitives.rs`
reproduce those, and `rust-tests/oracle_probe.rs` holds them to the reference.

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
`CommandOutput`: the document, the human lines, and the exit code.
`human::emit` writes the document, or the lines followed by the stale notice
and the single `source:` footer.

**Warnings are written when they happen.** A command writes a deprecation,
resync notice, or cache notice through `context.diagnostics`, so the line
survives a failure that follows it.

**Collaborators are explicit.** `Context` carries the clock, HTTP, configuration,
catalogue store, and diagnostics, so every command runs in-process against
fixtures.

**Credentials cannot be printed.** `config::Secret` and the transport's `ApiKey`
have no formatting that shows their value.

**No path that depends on input data can panic.** Failures carry one of the
five exit codes.

## The ForeverDB sync

A sync spends a budget of requests over a rolling 24 hours. The cap is 36
unless `EVERLEDGER_FOREVERDB_DAILY_REQUESTS` sets it. `attempt.json` records
what each attempt spent and when it ended; a spend counts for 24 hours, and an
attempt may send the cap less the spends still counting. A full sync needs
1,025 requests, so at the default cap it is a traversal spread over weeks, and
at a cap above 1,025 it is one attempt. Its correctness is an ordering either
way:

- The attempt journal is written before each request is sent, so a request is
  counted even if the process dies while it is in flight. The next reader
  dates that attempt's spend from its last request start.
- The checkpoint is written after each finished page and each finished detail.
- The index is published by atomic replace only when every detail is present.
- The budget is fixed when an attempt starts. Spends that expire while it runs
  leave the journal and add nothing to it.
- Request starts are 2.1 seconds apart at any cap.
- An interrupt during a sync closes the journal on a second thread and then
  ends the process as SIGINT ends one, whatever fails on the way, so a stopped
  process never keeps the lock. Outside a sync SIGINT has its default effect.
- One `flock` on `sync.lock` serializes syncs, including against the Python
  implementation on the same data directory.

`attempt.json` has one reader and one writer. The reader adds the spend of a
journal that records none for its own attempt (one written before spends
existed, or one left by a killed attempt) and drops expired spends; the writer
drops them again. `status` and `vendors` read the journal through that reader.

The cap is read once, before the arguments, into `config::DailyRequestCap`,
which cannot be zero. A value that is not a positive integer ends every
invocation as a usage error, whatever the command.

The connector transport runs each request through `foreverdb_connector.py`, a
Python process that calls the `dynamic_credentials` helper and performs the
request itself. The helper may replace the request rather than add a header to
it, and the surrogate credential never enters the Rust process.

## Verification

- `cargo test` runs the unit and integration tests, the golden vectors, the
  oracle probe, and the check that the README matches `--help`.
- `parity/` compares this binary with the Python reference: the replayed test
  corpus, a fuzz over random worlds, and the argument surface. See
  `parity/README.md`.
- `docs/rust-port-differences.md` lists every deliberate difference from the
  reference.
- The cargo feature `parity-seams` gives the differential tools a virtual
  clock, a sleep log, and a loopback ForeverDB address. Release builds contain
  none of it.
- `.cargo/config.toml` confines everything cargo runs to loopback.
