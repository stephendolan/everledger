# ForeverDB backend

The mapping follows read-only probes made by Muse on November 15, 2026 with
client dev-0256, build 1.60.1.70938. No live ForeverDB calls were made while
implementing or testing this change.

## Mapping decisions

Lists are `{data, meta}` at `/api/v1/vendors?limit=100&offset=0`; pagination
uses `meta.total` and offsets in steps of 100. Limit 500 was rejected live.
Build and total must stay consistent across the checkpoint's list pages; a
mismatch discards that checkpoint and restarts on the next eligible attempt.
Details are `{data: {...vendor, sells}}`. There is no extraction timestamp in
the observed envelope: `published_at` stays null, and local `fetched_at` records
sync completion. A multi-day sync is a traversal, not an upstream atomic snapshot.
The preserved one-day/seven-day cache-age rules measure time since index
completion, not each detail's observation age. At today's cap, early details may
be at least 28 days old when an index first publishes; completion does not
establish that every sell was recently revalidated.

Each sell's `seenInForever: true` maps to seen; otherwise `classicData: true`
maps to baseline; otherwise it is unverified. Vendor sightings never promote
sells. Eric's Red Dye has both flags false. `side` supplies ForeverDB faction;
ninja output keeps its existing faction-unavailable wording. Each seller row
retains `zone`, `zone_slug`, `mapId`, and a complete coordinate pair in `coords`
when available. `basis`, `sale_basis`, and `stack_price_copper` carry the sell
claims and price without duplicate `status`, `price_basis`, `price_raw`, or
`locations` fields.

`stack_price_copper` retains the integer total `price`, and `stack` retains the
purchase count. `price_copper` is exact per item only when the total divides
evenly by stack (including stack 1); otherwise it is null. The API's floating
`pricePerItem` is preserved in the raw sell but never rounded into copper.
Both `vendors` and `price` print the stack total/count for uneven division,
e.g. **10c per 3**. Exact divisions print per-item copper. `limited` and
`limitedStockCount` survive in the index, and human output marks limited stock,
including a count when supplied. Listed prices stay out of craft margins.

An unknown item has `vendors: null` with `reason: "item not in dataset"`.
Historical items whose last seller disappears retain `vendors: []` with null
reason. Human absence wording is bounded to the index's knowledge.

## Transport and activation

`EVERLEDGER_FOREVERDB_VIA_CONNECTOR=custom.foreverdb` selects connector-surrogate
transport, taking precedence over key settings. Only this mode lazily imports
`dynamic_credentials` from `EVERLEDGER_DYNAMIC_CREDENTIALS_DIR` (default
`/opt/muse/skills/custom-connector-setup/bin`) and calls
`add_surrogate_to_request(request, "custom.foreverdb", allowed_hosts=["foreverdb.net"])`.
It never reads a raw key. A missing helper gives a clear sync error; status,
help and other commands can still run without it. Status names the selected
transport without displaying credentials.

Env-key mode retains `X-API-Key`: nonblank `EVERLEDGER_FOREVERDB_API_KEY` wins
over `EVERLEDGER_FOREVERDB_API_KEY_FILE`, whose default is
`$EVERLEDGER_DATA_DIR/foreverdb-api-key`. Both transports send a browser
User-Agent and refuse redirects. Errors and cache files never include key or
surrogate material; upstream error messages/bodies are suppressed.

Activation requires configuration AND a complete index in the current format.
Old provisional indexes/checkpoints cannot activate. Removing configuration
restores ninja immediately. Lookup and status make no ForeverDB requests.
Transport/server failures can serve the prior index stale for up to seven days;
HTTP 4xx or older indexes select ninja fallback. A successful fallback after a
ForeverDB failure prints stale and exits 3; unavailable sources exit 4, matching
the existing contract. Ordinary vendor answers become stale after one day.

## Coverage

ForeverDB is not a superset of ninja. Of the four known Ironforge Red Dye
sellers, Poranna and Bombus are seen, Eric is unverified, and Burbik's sells
omit Red Dye. With ForeverDB active, `vendors` compares distinct NPC IDs with
the existing ninja item cache. `--in` scopes both sets before comparison.
When additional sellers exist it adds, for example:

```text
Fallback dataset knows 1 more seller for this item.
```

This is a data line; the single `source:` footer remains intact. It does not
merge fallback sellers or provenance into the answer, refresh ninja, or
change exit codes. JSON exposes `fallback_additional_sellers`. No comparison
cache means no claimed extra sellers. Cache entries for a different ninja URL
are excluded. Status reports each backend's vendor/item record counts, with
ninja vendors counted once across its cached items. These are local cache
counts, not a claim of complete ninja coverage.

## Sync request math and persistence

No per-vendor revision exists in the observed API. Reusing details based on
list metadata or build would miss changed sells, so every completed sync
fetches every detail. For N vendors, bootstrap AND steady state need
`ceil(N / 100) + N` requests: **11 + 1,014 = 1,025** today, requiring at least
**29 daily attempts** at the default cap of 36 requests each (28 × 36 + 17).
Set `EVERLEDGER_FOREVERDB_DAILY_REQUESTS` to a positive integer to override the
cap; unset defaults to 36, and invalid values are usage errors. Stephen's
November 16, 2026 decision selects `5000` for his environment. Unchanged data
costs the same. A five-vendor fixture needs 1 + 5 = 6 each time; a 40-vendor
fixture needs 41 total, split as 36 + 5 at the default cap. Empty datasets still
require one list request.

The configured cap is a rolling 24-hour request budget shared across attempts,
including failures and retries. Each attempt can spend only the remaining
budget; sequential request starts stay at least 2.1 seconds apart. A retryable
HTTP 502 gets one retry with an in-attempt wait between 60 and 300 seconds.
If `retry_after` exceeds 300 seconds or the budget prevents that retry, the
checkpoint retains `not_before` and the sync stays pending until a later
attempt can resume. A second failure ends the attempt. All retries add to the
total request math.

A lock serializes syncs; the attempt journal records each request before
transport, preserving request spend even on abrupt exit. Checkpoints retain finished
pages and details. Validated indexes publish with atomic replace only after
all details finish. Interruptions, errors and budget exhaustion retain the prior
index. The profession catalogue refresh still makes its existing ninja requests;
those do not count toward the ForeverDB cap.

## Offline verification

```bash
PYTHONPATH=python python3 -m unittest discover -s python/tests
```

Fixtures reconstruct the supplied real wire shapes rather than claiming full
response captures. They cover list metadata for 1,014 vendors at limit 100,
wrapped vendor 54 (Corina Steele, Gladius at 536c), the four Red Dye vendors
with Burbik's omission and Eric unverified, and per-sell precedence. Coordinates
and map IDs in these fixtures are representative; the NPC IDs match existing
ninja ground truth. Item 9001 and uneven-stack cases are synthetic regressions.
Tests cover exact prices and limited stock, cached coverage, status counts,
502 success/failure and budget-boundary resumption, stubbed dynamic_credentials
with no vault, outgoing UA in both transports, missing-module isolation, key
precedence/redaction (including helper import/injection exceptions), daily caps/pacing, checkpoint resumption, atomic publication,
stale fallback, and exact README/help agreement. Network integrations use only
local fixture servers; all ForeverDB responses are replaced.

## Muse: first connector sync on the VM

Keep the existing ninja cache so coverage can be compared. From the deployed
checkout (substitute its path if different):

```bash
cd /home/devbox/repos/everledger
export EVERLEDGER_DATA_DIR="${EVERLEDGER_DATA_DIR:-$HOME/.everledger}"
export EVERLEDGER_FOREVERDB_VIA_CONNECTOR=custom.foreverdb
export EVERLEDGER_DYNAMIC_CREDENTIALS_DIR=/opt/muse/skills/custom-connector-setup/bin
PYTHONPATH=python python3 -m unittest discover -s python/tests
./python/everledger status --json
./python/everledger refresh catalogue --json
./python/everledger status --json
```

Before completion, status should name connector-surrogate and ninja active.
Set `EVERLEDGER_FOREVERDB_DAILY_REQUESTS=5000` to use Stephen's chosen cap;
otherwise the default is 36. Each refresh reports at most the configured cap.
If unfinished, repeat the same refresh command with these environment settings
until `vendor_sync.complete` is true (at least 29 daily attempts at today's
count with the default cap). An immediate repeat can spend any budget still
available in the rolling window; it reports zero requests when that budget
is exhausted or a retry wait exceeds the in-attempt limit. Requests become
available again as recorded spends age out of the 24-hour window. Raising
`EVERLEDGER_FOREVERDB_DAILY_REQUESTS` makes the added budget available immediately.
Preserve the attempt and checkpoint files. No raw key export is needed.

After completion:

```bash
./python/everledger vendors 2604 --in IF
./python/everledger vendors 2604 --in IF --json
./python/everledger vendors 2488
./python/everledger price 2604
./python/everledger status --json
```

Inspect the sell labels and fallback coverage against the retained ninja cache.
Later full syncs have the same 1,025-request minimum at today's vendor count.

## Learnings

The list has no safe sell revision: honest steady-state freshness costs a full
detail traversal. Browser UA is required before authentication can succeed.
Sell-level provenance and dataset coverage answer different questions; observed
sales do not establish exhaustive seller coverage. Integer stack totals avoid
false precision from floating per-item prices.
