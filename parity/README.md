# Differential parity harness

Python 3 standard library only. Run from the repository root:

```sh
python3 parity/run.py parity/oracle-candidate
python3 parity/run.py target/debug/everledger --report /tmp/rust-parity.json
python3 parity/run.py target/debug/everledger --scenario live_checkpoint --scenario 'foreverdb/*502*'
python3 parity/run.py parity/oracle-candidate --list
python3 -m unittest parity.test_parity -v
python3 parity/argv_surface.py target/debug/everledger
EVERLEDGER_PARITY_ORIGIN=2026-11-15T12:34:56.789012Z python3 parity/run.py target/debug/everledger
```

The Rust executable needs the `parity-seams` feature. `oracle-candidate` is an
executable stand-in that runs the read-only Python oracle with those seams.
The runner exits 1 for any unexplained difference, fixture/assertion failure,
empty help output, credential leak, or empty scenario selection. It exits 0
only when every selected step passes. A full run takes roughly two minutes
on this host; the self-suite runs the full corpus twice.

Each source test maps to a named scenario. `--list` prints all 71 mappings;
the report includes the same accounting, selection, and CLI translation notes.
An inventory check fails if the reference test set changes. All 71 methods
have CLI equivalents, including the 23 ForeverDB methods that use internals.

## Scoreboard and report

| Column | Meaning |
| --- | --- |
| cases | CLI invocations, including the alternate rendering of each invocation |
| identical | All compared bytes and observations match after the two permitted normalizations |
| sanctioned | Differences exist, all covered by a named policy exemption |
| unexplained | At least one difference lacks an exemption |

Each sanctioned step prints its exemption class beside each affected field.
The JSON report includes the table, every step, both complete transcripts, all
differences, byte ranges with offsets, assertion errors, and the 71-test
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

`corpus.py` loads `tests/test_cli.py` as a fixture vocabulary and executes its
48 test methods. A subclass replaces `run_cli` and `sync_foreverdb` with paired
subprocess runs. Original assertions still run against the oracle result;
assertion failures fail the harness independently of parity. This preserves
loops, cursor chaining, route changes, file edits, environment edits, and
invocation order without duplicating the tests' logic. Test fixture `NOW` and
relative datetime mutations use the virtual origin.

Each scenario owns a new loopback HTTP server, data directory, and optional
connector stub directory. Both sides run sequentially at the **same absolute
paths and port**, with separate persistent snapshots. Fixture edits between
steps are applied to each side's state, translating only previously paired
random sync IDs. Every side's resulting snapshot is compared before any
fixture edit can replace it. Candidate state is never seeded from a completed
oracle invocation. The alternate human/JSON rendering forks each side's
pre-step state and discards its result after comparison; the replay rendering
alone determines subsequent state. The human and JSON defaults for `events`
remain the CLI's own defaults.

The two subprocess calls to `lookup(refresh=True)` in `test_cache_failure_window`
become public `refresh catalogue` calls with the same two-day/eight-day cache
states and 503 response. The refresh fixture also supplies the twelve profession
pages. Other in-process ForeverDB behavior uses `refresh catalogue`, `vendors`,
`status`, and the resulting cache files. The synthetic 40-vendor bootstrap
consumes 36 requests then resumes with 5. The extraction-change case consumes
36 full list pages before encountering a changed build on page 37. The retry
boundary case places a retryable 502 at request 36. The compile-time cap stays
36 throughout.

SIGINT and SIGKILL cases hold the third ForeverDB response after logging its
request, signal the running process group, wait for process exit, and release
the server. Their next invocations compare the old index, checkpoint, journal,
resumption, and cooldown through the CLI. The abrupt-exit case shifts the
recorded attempt-start timestamp to 24h05m and last-start timestamp to 23h55m
before the next origin, reproducing the
original test's clock movement without a cap override. Connector cases load
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

The candidate's `parity-seams` build implements these variables. Release
builds contain none of these overrides:

- `EVERLEDGER_SEAM_NOW`: UTC ISO-8601 origin with `Z` suffix. The corpus uses
  `2026-11-15T12:00:00Z`; `EVERLEDGER_PARITY_ORIGIN` gives the runner another
  origin, and a fractional one exercises sub-second age and pacing arithmetic.
- `EVERLEDGER_SEAM_SLEEP_LOG`: each sleep returns immediately, appends the
  Python `repr` of its float seconds plus a newline, and advances elapsed time.
- `EVERLEDGER_SEAM_FOREVERDB_URL`: loopback base replacing ForeverDB. A seam
  build fails closed if this is unset.

Elapsed time is an f64 starting at 0.0 for each invocation. Wall time is origin
plus elapsed, rounded half-even to microseconds; monotonic time is elapsed.
The oracle wrapper patches `datetime.datetime.now`, `time.monotonic`, and
`time.sleep`, then sets only `everledger_foreverdb.BASE`. No oracle source file
is edited. The private oracle elapsed-state file shares the clock with Python
children through `oracle_hook/sitecustomize.py`; this includes
`refresh.sh -> build_catalogue.py`, so generated catalogues use the same clock.
Startup-hook failure exits 125 and fails the harness. These private wrapper
variables are not additional candidate seams.

## Policy and self-proof

The only byte normalizations are generated top-level 32-hex JSON `sync_id`
values in ForeverDB's `attempt.json`, `pending.json`, and `index.json`, and their
Python atomic-write temporary filename suffixes. stdout, stderr, unrelated
files, and nested payload fields receive no normalization. Sync IDs retain their relationships
across files and invocations; changing one reference or starting an unexpected
new sync remains visible. Normalization preserves all other JSON formatting,
key order, whitespace, paths, numbers, and timestamps. Temporary-name
normalization collisions fail the harness.

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
and the sleep log. A further wrong candidate ignores the last request start
for abrupt-exit cooldown and must be rejected. The suite also compares the wrapper with plain `./everledger` on
three scenarios without time-dependent observations, tests clock inheritance
in a real Python child, exercises exact-query routing and redirect refusal,
and checks normalization and allowlist boundaries.

## Argument surface

`argv_surface.py` runs about a thousand argument vectors through the oracle and
a candidate and compares how each side classifies them: rejected by the argument
parser, answered with help, or accepted. It covers missing values, repeated and
abbreviated options, split positionals, negative numbers, and separators, which
the replayed tests barely touch. The port types command-line integers as i64 in
ASCII digits, so a candidate may reject the Unicode-digit and unbounded integers
that Python's `int()` accepts; the script reports each such case as sanctioned.

## Differential fuzz

`fuzz.py` builds random worlds inside the shapes the oracle's contract allows
(null where the oracle tolerates null, absent keys where it reads with a
default, every provenance spelling, paged and recoverable journals) and runs
random invocations through the same engine and comparison policy as the
replayed corpus. The corpus is fixed and can be fitted; the fuzz cannot.

```sh
python3 parity/fuzz.py target/seams/debug/everledger --cases 40 --seed 1
python3 parity/fuzz.py target/seams/debug/everledger --only fuzz-read/1/17 --report /tmp/case.json
python3 parity/fuzz.py target/seams/debug/everledger --commands events,sales,net
python3 parity/fuzz.py target/seams/debug/everledger --suite sync --cases 30
```

The `sync` suite drives `refresh catalogue` through several attempts per case:
random profession pages for the catalogue build, a random ForeverDB dataset of
up to 120 vendors, scripted failures at random request indices (retryable and
plain 502, 503, 401, 302, bodies that are not JSON or not the mapped shape),
and a day passing or not between attempts. Every step also asserts the fixture
key is absent from stdout, stderr, and every file.

A world never leaves the contract, so the oracle never fails internally on
one, and every difference is a finding. `--seed` and `--only` reproduce a case.
