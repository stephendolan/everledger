# Journalator consumer contract

The CLI adapts the roster service at one event boundary. Commands share this
adapter instead of each translating the service protocol.

## Watching and pagination

Start with `everledger events --limit 0 --json`. The service accepts only positive
limits, so the CLI drains a snapshot in pages of 500 and discards its entries.
Bootstrap may require multiple requests. It returns the service terminal
`checkpoint` unchanged as `after_cursor`; it does not write state to disk.

Poll with `everledger events --after "$checkpoint" --type purchase --json`.
When `has_more` is true, save `next_cursor` as temporary pagination progress and
pass it to `--cursor`. Only commit `after_cursor` as the next watch position when
`has_more` is false. A finished window has `next_cursor: null` and
`after_cursor == checkpoint`. Handle entries before committing progress for
at-least-once processing. A resync can replay entries.

Character, item, venue, and type selectors are local views over service requests
without those selectors. Changing a view does not change the service checkpoint
scope. Changing a filter only affects subsequent entries; it does not replay
entries before the checkpoint. Character selectors still resolve through the
roster character endpoint, and item matching retains ID/name fallback behavior.
Page cursors carry the service page cursor, local view, and recovery upper bound; treat them
as opaque and pass them back to this CLI. `--cursor` inherits its view and cannot
be combined with new selectors. Limits bound scanned rows, so a filtered page
can be empty while `has_more` remains true. Aggregates consume every page before
computing totals. Service `filters` metadata describes the wire query.

User time inputs remain `7d`, `24h`, dates, or ISO timestamps. Date and naive ISO
inputs use the local timezone. The roster HTTP boundary converts normalized
UTC timestamps to integer epoch seconds for `since` and exclusive `until`.
`resync_since` accepts ISO strings, decimal strings, and integer hints, including
zero; the retry uses the same normalization. `--after` excludes time inputs.

## Service capture and adapter readings

The service v1.3.0 capture taken on November 19, 2026 establishes the keys and
types below. Object contents are unspecified where the capture records only
an object. The capture contains one first-and-terminal events page with eight
entries, `has_more: false`, and `next_cursor: null`.

### `/characters`

| Location | Keys and captured types |
| --- | --- |
| Envelope | `generated_at`: string; `sources`: array; `characters`: array. |
| Each source | `available`: boolean; `file`, `mtime`: string; `globals`: object; `mtime_ns`, `size_bytes`: integer. |
| Each character | `class`, `faction`, `name`, `professions_updated_at`, `race`, `realm`, `short_name`: string; `class_id`, `gold_copper`, `level`, `race_id`: integer. |
| Character structures | `hearth`, `professions`, `sources`: object; `partial_data`: object or array. |

Each character's `professions` is an object. The adapter's character view
requires a list of professions, so `char` without a name exits 1 on this shape.
`recipe` catches that roster failure and reports known-by as not checked; a
matching recipe still returns its catalogue answer. A character list that
omits profession data can serve commands that only need names.

### `/characters/<name>`

| Location | Keys and captured types |
| --- | --- |
| Envelope | `generated_at`: string; `sources`: array with the source shape above; `character`: object. |
| Character scalars | `class`, `faction`, `name`, `professions_updated_at`, `race`, `realm`, `short_name`: string; `class_id`, `gold_copper`, `level`, `race_id`: integer. |
| Character structures | `hearth`: object containing string `location`; `partial_data`: empty array; `professions`: array; `sources`: object. |
| Each profession | `max_skill`, `skill`: integer; `name`: string; `recipes`: object; `skill_available`: boolean. |
| Character sources | `gold_copper`, `hearth`, `level`, `professions`: string. |

The adapter requires the `character` wrapper. Its detail profession list can
supply skill and recipe knowledge. The capture does not establish the inside
of `recipes`; the reference fixtures establish `available` and `known` spell
IDs.

### `/listings`

| Location | Keys and captured types |
| --- | --- |
| Envelope | `filters`: empty object; `generated_at`: string; `listings`, `scans`, `sources`: array; `returned_count`: integer. |
| Each listing | `auctionID`, `count`, `expirationTime`, `item_id`: integer; `character`, `name`, `realm`, `source`: string; `location`, `price`: object; `undercut`: null. |
| Each scan | `available`: boolean; `character`: string; `pricing`: object. |
| Each source | `available`, `stale`: boolean; `file`, `mtime`: string; `file_age_seconds`, `max_age_seconds`, `mtime_ns`, `size_bytes`: integer; `globals`: object. |

The adapter reads listing items from `item_id` and `name`. The inside of
`price` and `pricing` is established by the reference fixtures: recorded
prices live under `price.recorded`, with availability under `price.available`.
Scan `pricing.source` takes precedence over row `price.source`, which takes
precedence over envelope sources when selecting listing freshness sources.

### `/events`

| Location | Keys and captured types |
| --- | --- |
| Envelope flags | `available`, `complete`, `has_more`: boolean. |
| Envelope strings | `checkpoint`, `generated_at`, `mode`, `snapshot_at`, `snapshot_id`, `source_id`, `state`: string. |
| Envelope counts | `matched_count`, `returned_count`, `schema_version`: integer. |
| Envelope structures | `coverage`, `filters`, `item_matching`, `roster_enrichment`: object; `entries`: array; `next_cursor`, `reason`: null. There is no top-level `sources`. |
| Filters | `actor`, `item`, `until`, `venue`: null; `since`: integer; `types`: empty array. |
| Item matching | `basis`: null; `unresolved_linkless_count`: integer; `unresolved_recorded_names`: empty array. |
| Entry strings | `cash_basis`, `character`, `id`, `posting_basis`, `price_basis`, `recorded_at`, `type`, `venue`: string. |
| Entry numbers | `cash_copper`, `count`: integer; `deposit_copper`, `value_copper`: integer or null; `unit_copper`: float, integer, or null. |
| Entry structures | `actor`, `item`, `provenance`, `quote`, `raw`: object; `barter_items`, `currencies`: array or null. |
| Other entry fields | `cost_basis_eligible`: boolean; `counterparty`: string or null; `fee_copper`, `item_match_basis`, `payment_confirmed`, `proceeds_copper`: null. |

Service rows come from `entries`; each row's character comes from `character`
and its item comes from `item`. The outcome aggregate has a separate `events`
array before publishing `sales`, `purchases`, or `expirations`.

`matched_count` and `returned_count` pass through as sent. They describe the
service page, not the locally filtered array. An aggregate retains the terminal
page's counts; bootstrap retains them while printing no entries. In a loopback
fixture with eight service entries, filtering to three purchases still returns
both service counts as eight.

The service sends no top-level `sources` on the captured events page. The
journal save age therefore does not reach the `source:` footer. With no other
source, human events output ends with `source: unknown (age unknown)`;
`roster_enrichment` does not substitute for journal provenance.

## Contracts established by fixtures

The capture does not include a 409, any error body, an unavailable journal, or
inventory. It does not establish the inside of `item`, `price`, or `pricing`.
The reference fixtures in [test_cli.py](../python/tests/test_cli.py), the Rust
[outcome tests](../tests/outcomes.rs), and the [harness](../parity/README.md)
establish these readings:

| Surface | Adapter reading |
| --- | --- |
| Recovery | A 409 body with `error_code: cursor_invalid` and `resync_since` announces one resync and retries once. `snapshot_changed` restarts an aggregate once; a page consumer restarts from its committed checkpoint. |
| Errors | The HTTP boundary retains status and parsed response for recovery. Unrecognized error bodies keep the source failure. |
| Unavailable journal | `available: false` with optional `state` and `reason` supplies the unavailable result. It is distinct from an available empty page. |
| Inventory | `bags`, `bank`, `equipped`, and `mail` sections carry availability and item arrays. Rows require flat `item_id` and `count`; an optional nested item supplies the preferred item view. |
| Item contents | `id`, `name`, and optional `id_basis` support ID matching and name fallback. Missing and null fields remain distinct where the view uses them. |
| Price contents | `recorded` contains `buyout_copper`, `bid_copper`, `has_bid`, `deposit_copper`, and `expires_at`; `available` controls price availability. |
| Pricing provenance | Source time alternatives include `saved_at`, `scanned_at`, `mtime`, `updated_at`, and `fetched_at`; age alternatives include `scan_age_seconds`, `file_age_seconds`, and `age_seconds`. |

[Captured-roster verification](../parity/README.md#captured-roster-shapes) covers
the service-shaped public answers. Corpus and fuzz fixtures cover bootstrap,
epoch conversion, changing local views, multi-page aggregates, terminal
checkpoints, recovery hints, and stuck pagination. These are loopback contract
checks; the synthetic fixture values do not claim to reproduce the live journal.
