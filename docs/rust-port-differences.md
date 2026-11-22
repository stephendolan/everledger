# Differences between the binary and the Python reference

This register describes differences a caller can meet when switching between
the binary and the [Python fallback](../python/README.md). It also identifies
the policy a maintainer needs when reading a differential failure. A difference
outside this register is a defect.

The [matrix](../parity/README.md) compares each implementation with the
reference. Its named exemptions are rules, not blanket permission to change
output. [Build comparison](../parity/README.md#compare-two-builds) checks two
binaries' candidate transcripts without those exemptions.

## RestedXP guides

The Rust CLI renders the roster v1.4 character's `guide` field and provides
the `guide` command for routes, search, and the installed catalogue. The frozen
Python reference has no guide command or human guide rendering. A character payload without the new field retains the
existing Rust and Python behavior. With v1.4 payloads, Rust preserves the
server's guide data in JSON. Native read-path tests cover the new field and
command. The differential matrix continues to compare the shared surface
using its existing captured roster and replay fixtures.

## Diagnostics and help

| Difference | Boundary and instrument |
| --- | --- |
| Parser usage errors | Clap and argparse use different stderr text. `argv-surface` holds acceptance and exit classification; `corpus` also exercises usage failures. |
| Internal failures | The reference can print a Python exception or traceback; the binary names the failed boundary. `corpus` and `fuzz-read` allow selected stderr differences as `python-internal` or `shape-drift`. |
| Catalogue download errors | The reference invokes curl and can print its diagnostic. `corpus` permits selected `external-tool` stderr differences. |
| Help | The parsers produce different help text. `corpus` and `argv-surface` require successful, nonempty help; `tests/command_reference.rs` holds the binary's generated [reference](command-reference.md) byte for byte. |

CLI-defined failure messages, deprecation warnings, resync notices, and the
catalogue-age advisory remain exact in the corpus and fuzz instruments.

## Transport failures

Connection failures include the HTTP library's detail in parentheses. For
example, the reference can print `source unreachable (<urlopen error [Errno 111]
Connection refused>)` while the binary prints
`source unreachable (io: Connection refused (os error 111))`. The surrounding
text, exit status, and other observations still match. `status` can include
this detail in a source's JSON `state` or human output. The corpus and fuzz
engine permit only this delimited detail as `transport-detail`.

A response cut off inside its headers has a separate rule in `damage`. The
reference's HTTP library can accept a complete response with an empty body;
the binary treats the connection as broken. The reference then fails on the
empty body, usually with exit 1, or records a ForeverDB mapping failure. The
binary reports an unreachable source with exit 4, or uses an eligible stale
cache. `damage` requires each side to behave like its corresponding empty-body
or broken-connection case. Connector requests agree because both use Python.
Other malformed status lines, truncated declared bodies, and broken chunks
remain unreachable-source failures on both sides.

## Shape drift and cache files

Typed views refuse fields whose required type or presence does not fit the
boundary. The reference can compute an answer from the same payload or fail
inside a command. `corpus` and the fuzz instruments require matching exits for
their selected shape-drift cases while permitting a nonempty boundary
diagnostic. `damage` separately permits the binary to refuse a cache the
reference accepts: exit 1, empty stdout, no additional request, and the damaged
file unchanged.

| Cache | Examples held by `damage` |
| --- | --- |
| Attempt journal | `spends` must be a list of spends with a timestamp and whole nonnegative request count when sync reads it. `failed` is boolean and `status` is numeric or null. The reference can enlarge its budget with a negative spend or discard an expired spend unread. |
| Checkpoint | Read offsets and counts must be numeric, and mapped seller rows must satisfy their views. The reference can send a text offset to the service. |
| Index | Read items, counts, fetch time, and seller rows must satisfy the index view. The reference can answer a lookup that avoids a damaged field or spend requests before failing to rebuild the index. |
| Ninja cache | Required snapshot and seller fields retain their types and nullable output keys. The reference can print rows without validating every field and replace them on refresh. |
| Sync identity | Journal, checkpoint, and index `sync_id` values must be strings where decoded; the reference compares the values it finds. |

A command cannot replace a cache file it cannot decode. Where the reference
fails on damaged data, `damage` requires the binary to fail too. The journal
reader used by lookups does not validate sync-only spend fields; the
[architecture](architecture.md#the-foreverdb-sync) describes that boundary.

## Roster readings

The service v1.3.0 capture taken on November 19, 2026 establishes the
[journal and roster shapes](journalator-contract.md#service-capture-and-adapter-readings).
Unread keys remain in the ordered document unless the command explicitly
replaces or removes them.

| Rule | Reference-only reading | Binary behavior |
| --- | --- | --- |
| R1 | Endpoint `sources` as an object keyed by name | Requires a list, including on inventory responses. A null, scalar, or other non-list value also fails with `expected array`. |
| R2 | Entry `actor.character` | Ignores it and reads optional entry `character`. An unread `actor` can be a string without being decoded. |
| R3 | An unwrapped character detail | Requires the `character` wrapper. `character: null` exits 1 with decoder wording such as `invalid type: null, expected struct Character`. Recipe keeps its roster-failure fallback. |
| R4 | Page rows under `events` | Reads optional `entries`. An unread `events` value, even of another type, is not decoded as rows. Command aggregates have their own `events` array. |
| R5 | `matched`, `returned`, and `counts.matched`/`counts.returned` as counts to recompute | Leaves service counts untouched. `net` also retains `matched`, `returned`, and `counts` if the service sends them; the reference removes them there. |
| R6 | Listing `item` and flattened price or availability inputs | Reads `item_id`/`name` and `price.recorded`/`price.available`. It derives flattened output fields from nested price with the established defaults and insertion order. An unread flat `bid_copper` can be a string; nested recorded prices still require their types. |
| R7 | Entry-level `item_id`/`name` | Reads the optional `item` object. Unread flat values, including a numeric `name`, do not affect decisions or cause a type error. Inventory retains nested-first and flat item readings. |
| R8 | Text for a page cursor's item view | Requires an object and refuses a text item as a usage error, exit 2. |

The same unread-key rule applies to malformed values under R2, R4, R6, and R7:
being present in output does not mean the key supplies a typed decision.
Character and row views skip unread values rather than buffering them. They
can accept deep unread structures up to the document parser's limit, described
below; non-finite unread values still fail with a boundary diagnostic.

Both character endpoints carry a character's `professions` as a list of
profession objects, and the binary and the reference read it alike: `char`,
`recipe` with its known-by line, and `margins --char` agree. The reading was
tested against the capture file and against the live service on November 19,
2026; the shape is a list in `/characters` and in `/characters/<name>`.
`corpus` and `fuzz-read` hold it. The
[journal contract](journalator-contract.md#characters) describes the shape.

Instrument: `captured-roster` compares the service-shaped successful answers;
`corpus` and `fuzz-read` use the retained spellings. No matrix instrument
exhaustively exercises the rejected aliases or their wrong-type side effects.
`compare_builds.py` checks those effects byte for byte when its suites reach
them. The capture does not establish inventory, pricing interiors, unavailable
journals, or error bodies; their fixture-established readings remain supported.

## Non-finite numbers in successful responses

The reference accepts `NaN`, `Infinity`, and a float literal such as `1e400`
that overflows a double. The binary refuses them even under a key no command
reads. Loopback probes place each value in an unread response field:

| Source and command | Reference | Binary |
| --- | --- | --- |
| Roster page, `events --since 30d --json` | Exit 0; echoes the extra field. | Exit 1; roster boundary error. |
| AHledger quote, `price 2604 --json`, unread field inside `sales` | Exit 0; echoes that sales field. | Exit 1; AHledger quote boundary error. |
| ForeverDB list or vendor detail, `refresh catalogue --json` | Exit 0; completes the sync. Unread list/vendor fields are not copied into the mapped index. | Exit 4; failed sync with a mapping error in `vendor_sync`. |

The reference echoes non-finite values where the command retains the response;
`1e400` is emitted as `Infinity`. An unread top-level AHledger quote field is
dropped by `price` in the reference, but still causes the binary to refuse the
response. A ForeverDB sell's raw fields can survive in the index, while an unread field on the list or vendor itself does not.

Instrument: none for these successful-response cases. No parity instrument
sends these values in those fields, so the harness does not hold this
difference. Non-finite `retry_after` values in `corpus` and `interrupts` exercise
the separate error-body retry contract.

## Deep nesting

The document parser and typed decoder have separate limits. Views that buffer
unread keys through `#[serde(flatten)]` are limited by the typed decoder's
nesting counter; enclosing containers count toward it. These views are in
[src/foreverdb/mapping.rs](../src/foreverdb/mapping.rs). Other views skip unread
structures without that buffering limit.

Loopback probes use an empty array wrapped in more arrays under an unread
`extra` key. The reference returns exit 0 for all the depths below.

| Response and command | Binary observation |
| --- | --- |
| Roster page, `events --since 30d --json` | 126, 127, 200, and 256 nested arrays succeed with exit 0; 257 exits 1. |
| AHledger quote, `price 2604 --json` | The same tested depths and exits as the roster page. |
| ForeverDB list, `refresh catalogue --json` | 126, 127, 200, and 256 nested arrays succeed; 257 is a failed sync, exit 4. |
| ForeverDB vendor detail, unread field in `data` | 125 nested arrays succeed; 126 or more fail the mapping, exit 4. |
| ForeverDB sell, unread field inside a sell | 123 nested arrays succeed; 124 or more fail the mapping, exit 4. |

The command for both detail cases is `refresh catalogue --json`. The earlier
boundary for a sell reflects its enclosing object and `sells` array. The
reference accepts greater depths while its own recursion limit allows them.
These results concern unread values; required fields still have their own
shape rules.

Instrument: none. No parity instrument sends these depths, so the harness does
not hold either the buffered-view difference or the document-depth difference.

## Interrupts

Python handles Ctrl-C as an exception in the syncing thread. The binary closes
the journal on a second thread and ends as SIGINT ends a process. Both preserve
request spend. Their edge behavior differs:

| Signal timing or state | Binary behavior versus reference | Instrument |
| --- | --- | --- |
| Between an in-memory checkpoint change and its write | Saves its last written checkpoint; the reference saves its in-memory change. Resumption can fetch that page or detail again within budget. | None targets this exact gap; `recovery` holds requests and tests both signals and transports. |
| While recording a failure | Records interruption and retains its checkpoint; the reference leaves the journal and checkpoint at the point the exception lands. | None targets this exact gap. |
| Before the signal thread exists | Can leave the first journal write's temporary file; the reference unwinds and removes its temporary file. | None targets this exact gap. |
| SIGINT ignored at startup | Closes the journal and exits; the reference keeps ignoring the signal. | None. |

`interrupts` holds the shared cases of a lost stderr reader and a signal during
ninja refresh, and checks large retry delays. `recovery` and `corpus` hold
request-boundary interruption and subsequent resumption. Their timing coverage
does not imply coverage of every instruction between writes.

## Other input and rendering differences

| Difference | Instrument |
| --- | --- |
| Integer arguments, numeric item IDs, quantities, relative-time digits, epoch strings, and vendor price text use ASCII digits; Python accepts Unicode decimal digits. | `argv-surface` holds arguments and the daily cap; `tests/oracle_probe.rs` holds relative selectors and epoch strings. No instrument covers every response spelling. |
| Integer arguments, the daily cap, and quantities are bounded by their Rust integer types; Python integers are unbounded. | `argv-surface` holds argument and cap limits. |
| Unpaired UTF-16 surrogates cannot be Rust strings. | No differential instrument; the primitive probe skips unrepresentable records. |
| JSON response bodies use UTF-8, with an optional byte-order mark; Python also accepts UTF-16 and UTF-32. | None. |
| A negative amount rounding to zero copper prints `0c`; Python prints `-0c`. | `tests/oracle_probe.rs` explicitly normalizes this money expectation. |
| Vendor page `Last-Modified` lookup is case-insensitive; Python recognizes only that exact case. | None covers the difference; HTTP boundary tests hold case-insensitive lookup. |
| Case conversion follows the toolchain's Unicode tables and can differ from Python's Unicode version. | The primitive probe holds its recorded characters; no instrument exhausts characters outside that corpus. |

## Historical note

The binary is written against the Python implementation at commit `6f72b44`
and the differential harness holds it to that reference. It has been the live implementation since
November 18, 2026. Rare CPython exceptions for an extra date separator,
naive dates at years 1 and 9999, and interpreter-configurable decimal conversion
limits remain in the primitive-probe and argument-surface policies.
