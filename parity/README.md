# Differential parity harness

The harness compares the Rust binary with the Python reference using loopback
fixtures. It needs Python 3.11+, curl, and openssl for the TLS instrument.
Run from the repository root.

## Run every instrument

Build both seam binaries, then run the matrix:

```sh
cargo build --features parity-seams --target-dir target/seams
cargo build --release --features parity-seams --target-dir target/seams
python3 parity/matrix.py target/seams/debug/everledger target/seams/release/everledger
```

`matrix.py` finds the repository from its own location. Its arguments are the
debug seam binary, the release seam binary, and an optional log directory.
Binary and log paths are relative to the caller's working directory. Without
a log directory it creates a temporary one and prints its path. Logs remain
available when it exits. `MATRIX GREEN` means every selected instrument passes.
The full run includes long replays and a production-size sync. Concurrent
matrices can increase its runtime.

| Instrument | Check |
| --- | --- |
| `corpus` | Replay the reference suite through both commands. |
| `corpus-fractional` | Replay with a fractional clock origin. |
| `captured-roster` | Compare captured service keys and types through public commands. |
| `self-suite` | Prove the harness detects deliberately wrong candidates. |
| `argv-surface` | Compare argument acceptance and request-cap parsing. |
| `fuzz-read` | Generate random read-command worlds. |
| `fuzz-read-fractional` | Generate worlds with a fractional clock origin. |
| `fuzz-sync` | Generate syncs, failures, and budget boundaries. |
| `damage` | Compare damaged caches and broken HTTP responses. |
| `interrupts` | Signal syncs and exercise retry delays. |
| `handoff` | Let each implementation resume the other's files. |
| `tls` | Verify certificate trust and hostname checks. |
| `recovery` | Interrupt at every request on both transports. |
| `scale` | Compare the release build on a production-size fixture. |

Use repeatable `--only NAME` to select instruments. Timed signals and held
responses can be sensitive to concurrent work; rerun a failed instrument alone
with `--jobs 1` before treating it as a regression:

```sh
python3 parity/matrix.py target/seams/debug/everledger target/seams/release/everledger --only recovery --jobs 1
```

For individual scenarios and argument checks:

```sh
python3 parity/run.py parity/oracle-candidate --list
python3 parity/run.py target/seams/debug/everledger --scenario live_checkpoint --scenario 'foreverdb/*502*'
```

`oracle-candidate` runs the Python reference with the test seams. The replay
runner exits 1 for an unexplained difference, fixture failure, empty help,
credential leak, or empty selection. An inventory check maps every reference
test to a named scenario and fails when that set changes. The report includes
the inventory and CLI translation notes.

## Scoreboard and report

| Column | Meaning |
| --- | --- |
| cases | CLI invocations, including the alternate rendering of each invocation |
| identical | All compared bytes and observations match after the two permitted normalizations |
| sanctioned | Differences exist, all covered by a named policy exemption |
| unexplained | At least one difference lacks an exemption |

Each sanctioned step prints its exemption class beside each affected field.
The JSON report includes the table, every step, both complete transcripts, all
differences, byte ranges with offsets, assertion errors, and the reference-test
accounting. Bytes use `{"base64": "...", "length": N}` so invalid UTF-8 and
trailing whitespace survive. Decode with `base64.b64decode(value['base64'])`.
Request lists retain their order and exact path/query strings. The four logged
headers include null for absent headers. Environment and argv are recorded as
inputs; the comparison covers exit status, stdout, stderr, requests, sleep log,
every data-directory path and byte, and the permission bits of each file a
step wrote. Directory paths end in `/` with empty
bytes, preserving empty fixture directories too.

Reports contain disposable fixture credentials, absolute temporary paths, and
complete cache contents. They are deliberately large and are ignored by Git
at the default `parity/report.json` location.

## Replay and isolation

`corpus.py` loads `python/tests/test_cli.py` as a fixture vocabulary and executes its
CLI test methods. A subclass replaces `run_cli` and `sync_foreverdb` with paired
subprocess runs. Original assertions still run against the oracle result;
assertion failures fail the harness independently of parity. This preserves
loops, cursor chaining, route changes, file edits, environment edits, and
invocation order without duplicating the tests' logic. Test fixture `NOW` and
relative datetime mutations use the virtual origin.

Each scenario owns a separate loopback HTTP server, data directory, and optional
connector stub directory. Both sides run sequentially at the **same absolute
paths and port**, with separate persistent snapshots. Fixture edits between
steps are applied to each side's state, translating only paired
random sync IDs. Every side's resulting snapshot is compared before any
fixture edit can replace it. Candidate state is never seeded from a completed
oracle invocation. The alternate human/JSON rendering forks each side's
pre-step state and discards its result after comparison; the replay rendering
alone determines subsequent state. The human and JSON defaults for `events`
remain the CLI's own defaults.

The two subprocess calls in `test_cache_failure_window` become public
`refresh catalogue` calls with the same two-day/eight-day cache states and 503
response. The subprocess with a patched transport in
`test_refresh_sync_failure_reports_catalogue_and_spend_with_or_without_cache`
becomes `refresh catalogue` against a scripted 503, under the test's own
assertions. The refresh fixture also supplies the twelve profession
pages. Other in-process ForeverDB behavior uses `refresh catalogue`, `vendors`,
`status`, and the resulting cache files. Where the reference patches its cap,
the scenario sets `EVERLEDGER_FOREVERDB_DAILY_REQUESTS` for both sides. The
synthetic 40-vendor bootstrap spends the default cap of 36, is deferred, and
resumes with 5 a day later. The extraction-change and retry-boundary cases run
as the reference writes them, at a cap of 1, and then at the default cap on a
cleared cache: 36 list pages before a changed build on page 37, and a
retryable 502 as request 36.

A ForeverDB scenario's clock runs. Each invocation starts where the virtual
time of the one before ended, `tomorrow()` moves the clock a day on, and a
scenario can start an invocation at an instant read from the journal. The
reference's tests hold their clock still during an attempt, so a spend there
ends the instant it began. Here it ends seconds later, so the window scenarios
take their boundaries from the recorded `finished_at`: one second before a
spend is 24 hours old, and that second.

A step can also change the data directory while a response is held. The
write-failure scenario uses it: a directory takes the place of `pending.json`,
or of `attempt.json`, while the first request waits, so the rename that
publishes the next write fails where the reference's test patches its writer.

SIGINT and SIGKILL cases hold the third ForeverDB response after logging its
request, signal the running process group, wait for process exit, and release
the server. Their next invocations compare the old index, checkpoint, journal,
resumption, and spends through the CLI. The abrupt-exit case runs at a cap of
3 and starts its next invocation one second before the killed attempt's last
request start is 24 hours old, when its `attempted_at` already is: the spend
must still count, and one second later it must not. Connector cases load
an actual temporary `dynamic_credentials.py` through the documented variable.
Import, preparation, and `Failure` exceptions use a sentinel which must be
absent from stdout, stderr, and every data file on **both** sides.

## Fixture server

`fixture.py` implements exact path-and-query matching before path fallback,
and reuses the tests' stateful journal checkpoint contract. It serves the
observed ForeverDB list/detail fixtures and synthetic paginated datasets.
Responses can be scripted by one-based ForeverDB request index with status,
raw or JSON body, headers, and an optional held response. Scripts count only
`/api/v1/` requests, so profession downloads do not shift retry boundaries.
It records method, path/query, Authorization, User-Agent, X-API-Key, and
X-Surrogate-Test. Redirect tests require exactly one vendor request per attempt.
Fixture errors and failure to reach a held response fail the run.

Every URL supplied by the runner must be HTTP(S) on a numeric loopback address
or `localhost`. Non-loopback configuration is rejected before subprocess
launch. HTTP/HTTPS/ALL proxy variables point to a bound, non-listening loopback
port; NO_PROXY contains only loopback. External service defaults therefore
fail through the dead proxy. No production service is contacted.

## Seam contract

The candidate's `parity-seams` build implements these variables. Builds
without that feature contain none of these overrides:

- `EVERLEDGER_SEAM_NOW`: UTC ISO-8601 origin with `Z` suffix. The corpus uses
  `2026-11-15T12:00:00Z`; `EVERLEDGER_PARITY_ORIGIN` gives the runner another
  origin, and a fractional one exercises sub-second age and pacing arithmetic.
  A scenario whose clock has moved passes the moved instant.
- `EVERLEDGER_SEAM_SLEEP_LOG`: each sleep returns immediately, appends the
  Python `repr` of its float seconds plus a newline, and advances elapsed time.
- `EVERLEDGER_SEAM_FOREVERDB_URL`: loopback base replacing ForeverDB. A seam
  build fails closed if this is unset.

Elapsed time is an f64 starting at 0.0 for each invocation. Wall time is origin
plus elapsed, rounded half-even to microseconds; monotonic time is elapsed.
The oracle wrapper patches `datetime.datetime.now`, `time.monotonic`, and
`time.sleep`, then sets only `everledger_foreverdb.BASE`. When that module
refuses its configuration as it loads, the wrapper leaves the report to the
CLI. No oracle source file is edited. The private oracle elapsed-state file shares the clock with Python
children through `oracle_hook/sitecustomize.py`; this includes
`python/refresh.sh -> python/build_catalogue.py`, so generated catalogues use the same clock.
Startup-hook failure exits 125 and fails the harness. These private wrapper
variables are not additional candidate seams.

## Policy and self-proof

The only byte normalizations are generated top-level 32-hex JSON `sync_id`
values in ForeverDB's `attempt.json`, `pending.json`, and `index.json`, and their
Python atomic-write temporary filename suffixes. stdout, stderr, unrelated
files, and nested payload fields receive no normalization. Sync IDs retain their relationships
across files and invocations; changing one reference or starting an unexpected
sync remains visible. Normalization preserves all other JSON formatting,
key order, whitespace, paths, numbers, and timestamps. Temporary-name
normalization collisions fail the harness.

A difference confined to the transport detail in `source unreachable (...)`
is sanctioned as `transport-detail` on stdout and stderr, whatever the step's
policy; the surrounding bytes still have to match.

`engine.ALLOWLIST` is the single policy allowlist. Parser usage, Python-internal
exception text, external-tool diagnostics, and explicitly selected shape-drift
messages can exempt stderr differences. An explicitly selected shape-drift
case also permits changed stdout when the candidate provides a non-empty
boundary diagnostic and preserves the exit status. Contract Failure text, resync notices,
deprecation warnings, and the catalogue-age warning remain exact. An exemption
never absorbs a different exit status, request, sleep, or file, nor a candidate
that prints nothing where the oracle printed a diagnostic. Help wording
is exempt, while its exit status and non-empty stdout are required. Equal
bytes always score identical, even on an exemptible step.

The self-suite checks two consecutive full oracle-versus-oracle runs with zero
sanctioned entries, plus deliberately wrong executables for one stdout byte,
exit status, contract stderr, request order, query strings, one cache byte,
and the sleep log. A further wrong candidate dates a killed attempt's spend
from the attempt's start instead of its last request and must be rejected. The suite also compares the wrapper with plain `./python/everledger` on
three scenarios without time-dependent observations, tests clock inheritance
in a real Python child, exercises exact-query routing and redirect refusal,
and checks normalization and allowlist boundaries.

## Argument surface

`argv_surface.py` runs about a thousand argument vectors through the oracle and
a candidate and compares how each side classifies them: rejected by the argument
parser, answered with help, or accepted. It covers missing values, repeated and
abbreviated options, split positionals, negative numbers, and separators, which
the replayed tests barely touch. The binary types command-line integers as i64 in
ASCII digits, so a candidate may reject the Unicode-digit and unbounded integers
that Python's `int()` accepts, and may accept a small integer written in more
than the 4,300 digits CPython converts; the script reports each such case as
sanctioned.

The script also covers the one setting read before the arguments, the daily
request cap. It classifies some forty values of
`EVERLEDGER_FOREVERDB_DAILY_REQUESTS` on both sides as refused or accepted, and
requires a refused value to end a sample of argument vectors with the same exit
status, stdout, and stderr on both sides. The same sanctioned integer forms
apply to the cap.

## Differential fuzz

`fuzz.py` builds random worlds around the captured roster keys and the
spellings the adapter reads: list-shaped endpoint sources, wrapped character
details, listing `item_id`/`name` with nested prices, and journal `entries` with
nested items. It varies supported provenance timestamp and age fields, missing
or null optional values, and paged or recoverable journals. Inventory and
pricing internals retain their fixture vocabulary. Commands run through the
same engine and policy as the replayed corpus. Sorted choice lists keep a seed
reproducible across processes without a `PYTHONHASHSEED` pin.

```sh
python3 parity/fuzz.py target/seams/debug/everledger --cases 40 --seed 1
```

The `sync` suite drives `refresh catalogue` through several attempts per case:
random profession pages for the catalogue build, a random ForeverDB dataset of
up to 120 vendors, scripted failures at random request indices (retryable and
plain 502, 503, 401, 302, bodies that are not JSON or not the mapped shape),
and time passing between attempts: none, part of a 24-hour window, more than
one, or to within a second of the instant a recorded spend expires. Each
attempt runs under its own daily cap: unset, a value from 1 to 5000 in several
spellings, or sometimes one the CLI refuses. Scripted retry delays run below,
at, and above the 300 seconds an attempt will wait. Scripted failures are aimed at the cap's last request and
the one before it. Every step also asserts the fixture key is absent from
stdout, stderr, and every file.

A world never leaves the contract, so the oracle never fails internally on
one, and every difference is a finding. `--seed` and `--only` reproduce a case.

## Production size

`scale.py` builds a ForeverDB index of 1,014 vendors (about 35 MB) and a
checkpoint four details short of complete, both with the oracle's own mapping
and snapshot code. It compares lookups, `status`, and the final sync attempt
through the engine, and prints each side's wall time. It then runs a whole
sync in one attempt, which a daily cap above 1,025 allows: 1,025 requests, with
the checkpoint rewritten after each one as it grows to full size. The reference
takes several minutes over that attempt. The replayed corpus and the fuzz use
small datasets, so this is the only check of parity and speed at the size a
finished sync produces.

```sh
cargo build --release --features parity-seams --target-dir target/seams
python3 parity/scale.py target/seams/release/everledger
```

## Recovery after every request

`recovery.py` kills a sync at each of its six requests,
with SIGINT and with SIGKILL, on both transports, and compares what the next
attempts do with the journal and checkpoint left behind.

```sh
python3 parity/recovery.py target/seams/debug/everledger
```

## Interrupts and retry delays

`interrupts.py` stages what the runner cannot: it holds a process at a chosen
request and signals it. Each implementation must end by SIGINT, with its
journal closed, when the reader of its stderr has gone, and again when the
signal arrives after the sync, during the ninja refresh that follows an
incomplete one. The script also runs a retryable 502 through `retry_after`
values from seconds, across the 300 an attempt will wait, to numbers no date
can hold. Both sides must send the same requests, sleep the same, exit with the
same code, and leave the same files. Two of the largest delays run again at an
instant where a clamp computed with one float rounding too many lands a second
short of the reference's.

```sh
python3 parity/interrupts.py target/seams/debug/everledger
```

## Cache files no sync writes

The corpus and the fuzz only ever hand an implementation files it wrote itself.
`damage.py` puts other things in front of both: damaged checkpoints, indexes,
journals, and ninja cache files, and responses that break HTTP. It has three
parts.

**Chosen cases.** The wrong JSON type for a whole file, empty containers, bytes
that are not UTF-8, spends that do not add up. For each, both sides run the
commands that read the file and must exit with the same code, send the same
requests, and print the same stdout. Where the reference carries on with a
payload it could not have written and the candidate refuses it, the case is
named in `SANCTIONED` with its reason.

**Every field.** Each key of each object in the four files is in turn dropped,
set to null, and given another JSON type: several hundred damaged files. The
candidate must do what the reference does, or refuse. To refuse is to exit 1
with nothing on stdout, having sent no request the reference did not send and
left the damaged file as it was. A candidate that carries on where the
reference fails, or sends a request the reference does not, fails the check.
The summary line counts the refusals and the output lists them.

**Responses that break HTTP.** A status line that is not HTTP, a body or an
error body shorter than its declared length, a broken chunk, and a valid
response cut off after each byte of its head, from each source. Both sides must
exit the same, send the same requests, and print the same stdout. One
difference is accepted: where a response ends inside its headers, the
reference's HTTP library reads a complete response with no body and the
candidate's reads a broken connection. Such a case passes only if the reference
does exactly what it does for an empty response and the candidate does exactly
what both do when the connection closes without a response.

```sh
python3 parity/damage.py target/seams/debug/everledger
```

## TLS

Every other check reaches its fixture over plain HTTP, so none of them runs a
handshake or verifies a certificate. `tls.py` mints a throwaway certificate
authority, gives the fixture a certificate for `localhost`, and has both
implementations fetch over TLS. With the authority in `SSL_CERT_FILE` both
sides must give the same answers: a vendor lookup, a price, status, a catalogue
rebuild, and a ForeverDB sync by key and by connector. Without the authority,
and again when the fixture is addressed by a name its certificate does not
carry, each side must end with exit code 4 and no request may reach the
fixture. The check needs the `openssl` command.

```sh
python3 parity/tls.py target/seams/debug/everledger
```

## One data directory, two implementations

The binary and Python fallback share a data directory. The replay runner
gives each side its own state copy, so `handoff.py` checks sequential use of
one directory: either implementation finishes the other's sync, reads its
published index, or waits for its sync lock. It exercises different caps both
back to back and a day apart.

Fallback directories can contain files from earlier reference builds.
`handoff.py` extracts the commits listed in `EARLIER` from local Git history,
creates complete and partial syncs, and caches ninja lookups. The current
reference and candidate must read and refresh each directory with matching
answers and files. A reference pin update adds the commit it replaces to
`EARLIER`. Indexes carrying unused `details` and `sync_requests` fields remain
part of this compatibility check.

```sh
python3 parity/handoff.py target/seams/debug/everledger
```

## Compare two builds

The matrix compares each build with the Python reference by policy. That can
exempt stderr wording or allow the binary to refuse data the reference accepts.
`compare_builds.py` compares the two builds' candidate transcripts byte for byte
on those paths too, after masking loopback ports, temporary paths, generated
sync IDs, and atomic-write suffixes. It runs the corpus and read fuzz at both
clock origins, plus sync fuzz. Both binaries must be files named `everledger`,
because the argument parser prints its own file name.

```sh
python3 parity/compare_builds.py target/seams/debug/everledger target/seams/release/everledger
```

Use paths to the two seam builds to compare. An optional third argument chooses
the log directory; otherwise the script prints a fresh temporary directory.
Every suite pair must have identical steps, exit codes, output, requests,
sleeps, files, and file modes. A difference or failed suite exits nonzero.
`BUILD PAIRS IDENTICAL` means every pair matches.

## Captured roster shapes

`tests/fixtures/captured-roster.json` holds roster responses with the keys and
JSON types of the service capture, and the commands to run over them.
`captured_roster.py` runs each command in both renderings through the
reference and the candidate, which must exit 0 with identical stdout and send
the same requests. It also checks listing key order and journal pass-through
fields. The [journal contract](../docs/journalator-contract.md) distinguishes
captured structures from fixture-established internals and explains the
list-professions shape the adapter refuses.

```sh
python3 parity/captured_roster.py target/seams/debug/everledger
```

`tests/golden/captured-roster.json` records what the reference did with each
command: its exit code, stdout, and requests. `tests/read_paths.rs` replays
that file in-process, so `cargo test` holds the binary to the reference on
these shapes without running Python. The instrument fails when the file does
not match the reference. After changing the fixture, regenerate it:

```sh
python3 parity/captured_roster.py target/seams/debug/everledger --write
```
