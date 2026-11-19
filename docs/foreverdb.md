# ForeverDB backend

ForeverDB supplies client vendor data. Ninja remains the vendor backend until
ForeverDB credentials are configured and a complete index is available.
ForeverDB lookups and status checks read local files; only `refresh catalogue`
sends requests to this backend.

## Mapping

The mapping accepts the API shapes observed with client `dev-0256`, build
`1.60.1.70938`. List responses at `/api/v1/vendors?limit=100&offset=0` contain
`data` and `meta`. Pagination uses `meta.total` and offsets in steps of 100.
The API rejects limit 500. Build and total must agree across a checkpoint's
list pages. A mismatch discards the checkpoint so another attempt can restart.
Detail responses wrap the vendor and `sells` in `data`.

The response envelope has no extraction timestamp. `published_at` stays null;
local `fetched_at` records sync completion. A sync spanning multiple attempts
is a traversal rather than an atomic upstream snapshot. Cache ages measure
from completion, so early details can be much older than the published index.

A sell with `seenInForever: true` maps to seen. Otherwise, `classicData: true`
maps to baseline; neither flag means unverified. A vendor sighting does not
promote its sells. `side` supplies faction. Each seller retains `zone`,
`zone_slug`, `mapId`, and a complete coordinate pair when available.
`basis` and `sale_basis` carry the sell claim.

`stack_price_copper` retains the integer total price and `stack` the purchase
count. `price_copper` is per item only when the total divides evenly by stack;
otherwise it is null. The floating `pricePerItem` survives in the raw sell and
is never rounded into copper. Both `vendors` and `price` print an uneven stack
as its total and count, for example **10c per 3**. Exact divisions print per-item
copper. `limited` and `limitedStockCount` survive in the index; human output
marks limited stock with its count when supplied. Listed prices stay out of
craft margins.

An item absent from the index has `vendors: null` and
`reason: "item not in dataset"`. An indexed item whose last seller disappears
retains `vendors: []` with a null reason. Neither result claims exhaustive
knowledge of sellers in the game.

## Transports and activation

Key transport sends `X-API-Key`. A nonblank `EVERLEDGER_FOREVERDB_API_KEY` takes
precedence over `EVERLEDGER_FOREVERDB_API_KEY_FILE`, whose default is
`$EVERLEDGER_DATA_DIR/foreverdb-api-key`. A missing key file means unconfigured;
an unreadable file is an error.

Set `EVERLEDGER_FOREVERDB_VIA_CONNECTOR=custom.foreverdb` to select
`connector-surrogate` transport instead. Any nonblank connector slug takes
precedence over key settings and avoids reading the raw key. This transport
runs `python3` with the bundled shim. It imports `dynamic_credentials` from
`EVERLEDGER_DYNAMIC_CREDENTIALS_DIR`, which defaults to
`/opt/muse/skills/custom-connector-setup/bin`. The helper's
`add_surrogate_to_request` receives the connector slug and
`allowed_hosts=["foreverdb.net"]`. A missing helper fails the sync; help and
status do not import it.

Both transports send a browser User-Agent and refuse redirects. Helper output
and upstream error bodies are suppressed so diagnostics and cache files do not
expose credentials. [Architecture](architecture.md#the-foreverdb-sync) describes
the connector process boundary.

Run `everledger refresh catalogue` with the chosen transport to build the
index. Before it completes, `status` names the transport and reports ninja as
active. Incomplete refreshes resume on later attempts. A completed refresh
reports `vendor_sync.complete: true`; status selects ForeverDB and reports
vendor and item record counts for each backend. Only a complete index in the
accepted format can activate. Removing the transport configuration selects
ninja.

Ordinary vendor answers become stale after one day. Following a failed sync,
a prior index is usable for up to seven days when the failure has no HTTP
status or a status of at least 500. Other failures select ninja fallback. A
successful fallback following a failed sync prints stale and exits 3;
unavailable sources exit 4. An older index without a recorded sync failure
remains eligible, with its age marked stale.

## Coverage against the ninja cache

ForeverDB is not a superset of ninja. `vendors` compares distinct NPC IDs in
its answer with those in the retained ninja item cache. `--in` scopes both sets.
If the cache contains additional sellers, human output reports their count;
JSON exposes `fallback_additional_sellers`.

The comparison does not merge sellers or provenance into the answer and does
not refresh ninja. It leaves the source footer and exit code intact. Without
a matching cache entry it makes no claim about additional sellers. Entries
for another ninja base URL are excluded.

Status counts local vendor and item records for each backend. It counts each
ninja vendor once across cached items. These counts describe cached coverage,
not the full ninja dataset.

## Requests and the rolling budget

The API supplies no per-vendor sell revision. Every full sync fetches every
detail, even when list metadata is unchanged. For `N` vendors the request count
is `max(1, ceil(N / 100)) + N`, before retries. For example, 1,014 vendors need
11 list requests plus 1,014 details, or 1,025 requests. An empty dataset still
needs one list request.

`EVERLEDGER_FOREVERDB_DAILY_REQUESTS` defaults to 36. A positive integer
overrides it. Invalid values fail every invocation with usage exit code 2,
including help. The budget spans a rolling 24 hours across attempts and
includes failures and retries. One refresh can spend only the allowance left
at its attempt's start. Repeating a refresh does not reset it. Raising the cap
makes the extra allowance available to the next attempt immediately.

At a cap of 36, the example sync needs at least `ceil(1025 / 36) = 29`
attempts with enough budget available. Retries add requests. Profession-page
downloads and ninja cache refreshes do not count against the ForeverDB
allowance. The [sync architecture](architecture.md#the-foreverdb-sync) defines
spend timestamps, pacing, and retry waits.

## State between attempts

Keep `vendors/foreverdb.net/` under `EVERLEDGER_DATA_DIR` when resuming.
`pending.json` holds completed list pages, fetched details, and a pending
retry's `not_before`. `attempt.json` holds request spend and the last attempt's
result. `index.json` holds the complete published index. Budget exhaustion,
errors, and interruptions retain the prior index; partial work cannot replace
it. A metadata mismatch discards the incompatible checkpoint.

The [sync architecture](architecture.md#the-foreverdb-sync) owns the locking,
journal write order, interrupt handling, and atomic publication rules. These
rules also allow the binary and Python fallback to resume each other's work.

## Offline verification

From the repository root:

```sh
cargo test --all-features
PYTHONPATH=python python3 -m unittest discover -s python/tests -v
```

Rust integration tests in [foreverdb.rs](../tests/foreverdb.rs) and
[rolling_budget.rs](../tests/rolling_budget.rs) exercise mapping, stack prices,
coverage, transport selection, and resumption. They check rolling-window
boundaries, retries, atomic publication, and stale fallback with fixture
responses. The Python suite covers the reference implementation. The
[parity matrix](../parity/README.md) compares both implementations through their
public commands, including interrupted syncs and shared-directory recovery.

[Fixtures](../tests/fixtures) reconstruct observed wire shapes. They include
list metadata for 1,014 vendors at limit 100, wrapped vendor 54 (Corina Steele,
Gladius at 536c), and four Red Dye vendors with Burbik's omission and Eric's
unverified sell. Coordinates and map IDs are representative; item 9001 and
uneven-stack cases are synthetic. These files are fixtures rather than full
response captures. Network checks use only loopback servers.
