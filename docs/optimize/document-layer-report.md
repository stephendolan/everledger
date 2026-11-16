# Production-size document layer

The release binary meets every production-scale timing target and preserves all
12 scale steps byte for byte. The dependency set and release profile are unchanged.

## Measurements

`cargo build --release --features parity-seams --target-dir target/seams`

`python3 parity/scale.py target/seams/release/everledger`

```
invocation                          oracle s  candidate s   ratio
vendors 16529 --json                    0.52         0.47    0.89
vendors 16529 --in IF                   0.49         0.46    0.94
vendors 16840                           0.49         0.46    0.93
vendors 999999 --json                   0.48         0.46    0.96
status --json                           0.53         0.47    0.89
refresh catalogue --json                6.86         2.42    0.35
steps 12, identical 12, sanctioned 0, unexplained 0, harness failures 0
```

The original binary at `99c828470b63b90919fe5966211e7432d455088e` measures:

```
invocation                          oracle s  candidate s   ratio
vendors 16529 --json                    0.51         1.33    2.60
vendors 16529 --in IF                   0.52         1.34    2.57
vendors 16840                           0.49         1.31    2.68
vendors 999999 --json                   0.51         1.33    2.62
status --json                           0.54         1.35    2.52
refresh catalogue --json                6.83        13.68    2.00
steps 12, identical 12, sanctioned 0, unexplained 0, harness failures 0
```

A second final run has read ratios of 0.87–0.95 and a refresh ratio of 0.38,
with 12 identical steps. These are two wall-clock samples on the same deterministic
production-size dataset; no separate holdout dataset or confidence interval is used.
All six targets pass both runs.

Peak RSS is measured by wrapping each child in `/usr/bin/time -f 'peak_kb %M'`,
with its output in a separate file, through `Engine.capture`. Both children receive
the same index from `parity.scale.oracle_state` and run `vendors 16529 --json`.
The fixture parent is excluded from each child's peak:

```
python peak_kb 192868
candidate peak_kb 161088
```

The original candidate peaks at 393,716 KB beside Python's 192,864 KB.
The final candidate uses 31,780 KB (16.5%) less peak memory than Python
and 232,628 KB (59.1%) less than the original candidate.
The [memory script](/home/codex/.bb-machines/tuple-mac-mini.tail767175.ts.net/thread-storage/thr_yidmn5hezj/document-layer-evidence/memory.py), [confirmation output](/home/codex/.bb-machines/tuple-mac-mini.tail767175.ts.net/thread-storage/thr_yidmn5hezj/document-layer-evidence/confirmation-scale.log),
and [experiment log](/home/codex/.bb-machines/tuple-mac-mini.tail767175.ts.net/thread-storage/thr_yidmn5hezj/document-layer-evidence/experiment-log.yaml) are in durable thread storage.

## Implementation

- Typed serde decoding traverses the ordered document directly. Scalar numeric
  edge cases use serde_json's existing converters. Ignored values still reject
  non-finite numbers, and decoded containers retain serde_json's recursion limit.
- The writer appends to one buffer in all four modes. String escaping, whitespace,
  Unicode handling, float spelling, and insertion order remain oracle-compatible.
- Parsed object keys share `Arc<str>` storage within the document. Integers that fit
  i64 stay inline; arbitrary integers retain their decimal spelling. Parsed vectors
  release unused capacity, and decoded sequences allocate their known length.
- A wide object's duplicate-key lookup uses a hash index after 32 entries, retaining
  the original insertion slot. Ordinary objects compare interned key pointers.
- Payload edits retain an undo entry for the affected field and decode before
  committing the view. Failure restores the exact document and key order. Nested
  detail insertion and retry removal avoid cloning the checkpoint.
- Scalar decoding dispatches by JSON type while preserving the previous untagged
  enum's variant choice, including u64 values beyond i64 becoming floats.

The single-buffer/direct-decode/transaction variant measures 0.86–0.92 seconds
for reads and 4.83 seconds for refresh. Compact storage and direct enums bring
reads to 0.57–0.61 seconds and refresh to 3.12 seconds. Direct scalar dispatch
reduces measured IndexData decode from 165.7 ms to 60.9 ms and clears the read target.
These are integrated measurements; the individual changes are not assigned additive gains.
A trial of full LTO and one codegen unit is not retained: it offers smaller savings
and the current source meets the targets with the existing build profile.

A stage measurement on the oracle-built 35 MB index reports file read 25.2 ms,
parse 228.4 ms, typed decode 60.9 ms, and dumps 149.3 ms. Parsing is the largest
remaining measured stage. CPU hardware counters are unavailable on this host
(`perf_event_paranoid=4`); the evidence uses elapsed time and process RSS.

## Verification

Every listed check exits zero. Commands use only loopback fixture servers.

- `cargo test --all-features`: 78 tests including the doc test pass, zero failures.
  Four new unit tests compare direct typed decoding with the previous serde JSON
  boundary, exercise probe documents and deep nesting, verify scalar variant
  selection against the original derive, and check failed payload edits and order.
- `cargo test --all-features --test oracle_probe -- --nocapture`:
  `json checked 4040 sanctioned 0 failed 0`; every probe category has zero failures.
- `cargo fmt --check`: no output.
- `cargo clippy --all-targets --all-features -- -D warnings`:
  `Finished dev profile [unoptimized + debuginfo]` with no warnings.
- `python3 parity/run.py target/seams/release/everledger --report /tmp/everledger-worker6/corpus-whole.json`:
  599 steps, 574 identical, 25 sanctioned, zero unexplained and zero harness failures.
- `EVERLEDGER_PARITY_ORIGIN=2026-11-15T12:34:56.789012Z python3 parity/run.py target/seams/release/everledger --report /tmp/everledger-worker6/corpus-fractional.json`:
  the same 599/574/25/0 totals, zero harness failures.
- `python3 parity/fuzz.py target/seams/release/everledger --cases 60 --seed 1 --report /tmp/everledger-worker6/fuzz-read.json`:
  `steps 1680, unexplained 0, harness failures 0`; all 1680 steps identical.
- `python3 parity/fuzz.py target/seams/release/everledger --suite sync --cases 60 --seed 1 --report /tmp/everledger-worker6/fuzz-sync.json`:
  `steps 962, unexplained 0, harness failures 0`; all 962 steps identical.
- `python3 parity/argv_surface.py target/seams/release/everledger`:
  `cases 1016`, `sanctioned 2`, `mismatches 0`.
- `python3 scripts/check_foreverdb_recovery.py target/seams/release/everledger`:
  `refresh {'cases': 96, 'identical': 84, 'sanctioned': 12, 'unexplained': 0}`;
  `24 signal/transport/request combinations: PASS`.
- `git diff --check`: no output.

The relevant corpus scoreboard is identical under both clock origins:

```
origin                       command   cases identical sanctioned unexplained
2026-11-15T12:00:00Z          refresh     115       112          3           0
                             status       55        54          1           0
                             vendors     105       104          1           0
2026-11-15T12:34:56.789012Z   refresh     115       112          3           0
                             status       55        54          1           0
                             vendors     105       104          1           0
```

## Scope and boundaries

The owned changes are `src/document.rs`, its new decoder and tests,
`Payload` in `src/http.rs`, and checkpoint update call sites in `src/foreverdb.rs`.
The additional foundation changes are:

- `src/primitives.rs`: the measured scalar visitor replaces repeated untagged
  variant failures, retaining the same variants and accepted shapes.
- `src/time.rs`: one match arm reads the new inline integer into `EpochSeconds`.

The document API exposes object keys as `Arc<str>` instead of `String` and
adds `Number::Small(i64)`. `Document::set` accepts `AsRef<str>` to support the
shared keys without command changes. Payload gains transactional `set_path`
and `remove`. Typed views retain their required and optional fields.

The Python oracle, reviewer-owned parity instruments, probe fixtures, sanctioned
differences, and request/write ordering are unchanged. No sanctioned difference
is proposed and no dependency is added. `RUST_API.md` is absent in this checkout;
the source and existing boundary tests supply the API reference.

## Learnings

Removing a JSON roundtrip leaves serde's untagged scalar dispatch as a substantial
cost: string scalars construct errors while trying earlier variants. A visitor
preserves the type model without those failed attempts. Borrowed document decoding
must preserve rejection of non-finite values even in ignored fields, numeric-key
rules, exact float conversions, and recursion limits. Shared keys and exact vector
capacity are necessary for the document plus typed view to fit below Python's RSS.
