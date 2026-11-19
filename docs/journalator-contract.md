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

## Response shapes

Service pages carry rows under `entries`; the command aggregate carries rows
under its own `events` key before publishing `sales`, `purchases`, or
`expirations`. An entry's character comes from `character`, and its item comes
from `item`. Listings use `item_id` and `name` on the row, with recorded prices
under `price.recorded` and availability under `price.available`. A character
detail response carries the `character` wrapper. Endpoint `sources` are lists;
source timestamp and age alternatives inside pricing remain supported.

Service counts pass through unchanged, including `matched_count` and
`returned_count`. They describe the service response, not the locally filtered
array. An aggregate retains the terminal page's counts, and bootstrap retains
them even though it prints no entries.

## Learnings

The first live service check on 2026-11-15 exposed fixture assumptions that did
not match the wire contract: zero limits, ISO query bounds, filtered checkpoint
reuse, and a terminal cursor in `next_cursor`. Keep fixtures strict about service
input bounds and separate terminal `checkpoint` from snapshot `next_cursor`.

Scope-bound checkpoints cannot implement changeable filters by forwarding those
filters. Keep the service stream unfiltered and carry local view state through
snapshot pagination. Never substitute a checkpoint for a page cursor or persist
a page cursor as a completed watch position.

The fixture suite covers bootstrap, epoch conversion, changed views while new
entries arrive, multi-page aggregates, terminal checkpoints, two auction
purchases totaling 2,094 copper, and the two reported listing prices (2,499 and
4,298 copper). It also checks resync hints and refuses stuck pagination.
The other six sample event rows are synthetic; these are contract fixtures,
not a capture of the full live journal.

No live verification was performed from this checkout. The supplied environment
evidence reports HTTP 403 “Peer not allowed” from the homeserver host. Muse must
verify the built CLI against the Mac service after this commit lands.
