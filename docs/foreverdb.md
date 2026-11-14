# ForeverDB backend and key-day acceptance

Implemented locally without a key or a live ForeverDB request. API access is
pending. The fixture suite verifies behavior against a synthetic contract; it
does not establish compatibility with the actual service.

## Mapping assumptions — all provisional

The wire assumptions live in `everledger_foreverdb_mapping.py`.

- **PROVISIONAL:** List requests use `/api/v1/vendors?page=1&per_page=100`, then
  consecutive numeric pages. The envelope has `vendors`, explicit `next_page`
  (`null` terminates), `build`, and ISO `extracted_at`. All pages describe the
  same extraction. No `seen=true` filter is sent: Classic-only vendors must also
  be indexed. Unfiltered completeness requires live confirmation.
- **PROVISIONAL:** Each list vendor has a positive integer `id` and a nonempty
  `updated_at` revision. That revision covers **every** sell addition/removal,
  price, label and location change. This is the critical assumption permitting
  incremental sync; missing revisions fail closed. If the real list cannot
  expose this, steady-state detail reuse is unsafe and needs redesign before
  activation. Build/extraction metadata alone does not prove unchanged sells.
- **PROVISIONAL:** `/api/v1/vendors/{id}` returns an unwrapped object with `id`,
  `name`, optional `title`, optional `seen`, `locations`, and a **complete**
  `sells` list. Each sell has integer `item_id`, optional item `name`,
  `price_copper` (nonnegative integer copper or explicit null), and optional
  `seen`. Null price stays unknown. No purchase quantity or faction is inferred.
- **PROVISIONAL:** Vendor and sell `seen` values are independent booleans:
  `true` means seen-in-Forever, `false` explicitly means Classic-only, and
  missing/null means unverified. Sell labels map to each seller row's `basis`,
  `sale_basis` and `price_basis`; vendor labels map only to `vendor_basis`.
  A vendor sighting cannot promote its sells. String/numeric labels fail the
  mapping rather than being coerced. The live gate must confirm whether the
  real labels describe sightings, sells, prices, or a different scope.
- **PROVISIONAL:** Locations are an ordered list of objects with `zone`,
  `zone_slug`, `x`, `y`; coordinates are percentages or null. Every object is
  retained in `locations`; every complete coordinate pair is also retained in
  the existing `coords` list. The first location supplies the legacy sorting
  `zone`/`zone_slug`. Zone filtering checks all locations and human output shows
  them all. Verify actual location fields, coordinate units and multiplicity.
- **PROVISIONAL:** The synthetic fixtures' field names, envelope, revision
  strings, labels, location multiplicity and sell membership are illustrative,
  not captured API observations. The four Red Dye seller names and listed 50c
  are regression expectations; their real ForeverDB records still need checking.

The transport uses the documented `https://foreverdb.net` base and `X-API-Key`.
Only vendor endpoints are requested; `/items` is never called. Mapping errors
preserve the active index and report that the provisional shape needs checking.

## Sync and request counts

`everledger refresh catalogue` extends the shipped refresh path. It still
rebuilds the profession catalogue. With a key, it also runs one vendor sync;
without an active ForeverDB index it refreshes the previously queried ninja
items as before. Lookups and status never request ForeverDB data.

Let **P** be the number of list pages, **N** the vendor count and **C** the count
of new or revised vendors. Bootstrap requires **P + N** requests; a later full
list traversal requires **P + C**, or **P** when nothing changed. Removed vendors
require no detail request. An actual production total cannot be known without
API access; no claim that the first sync fits a few dozen requests is made.

Each attempt allows at most **36 requests**, including failed requests, followed
by a **24-hour cooldown after completion/failure**. Requests are sequential and
paced at least 2.1 seconds apart (below 30/minute). There are no automatic retries.
A file lock serializes concurrent refreshes; an attempt journal preserves the
cooldown across processes and interruptions; the last request start is journaled
before transport so an abrupt process stop cannot shorten it. Bootstrap/large updates span daily
attempts, preserving completed list pages and details in a checkpoint. Approximate
bootstrap duration is `ceil((P + N) / 36)` daily attempts; interrupted requests
can add to that total. Steady-state churn or pagination can also require multiple
days. This bounds spend even if the revision assumption proves false.

Fixture counts: 4 vendors across 2 pages require **6** requests initially,
**2** unchanged, **3** with one changed vendor. A 40-vendor, one-page fixture
requires **41** total: **36 + 5** across two daily attempts. The CLI JSON reports
requests for each attempt and cumulative requests for the completed transaction;
status reports the last attempt's actual count. The 12 existing profession-page
requests are to ninja and are not ForeverDB API requests.

Validated checkpoints and the completed index are written through temporary
files, flushed, then atomically replaced. Only a complete index activates;
errors, interrupts and budget exhaustion leave the prior index intact. The
checkpoint is separate from the active index. If pagination resumes into a new
extraction, the obsolete checkpoint is discarded and the next eligible daily
attempt restarts listing; it cannot trap all later attempts. Historical item records remain
with an empty seller list after their final seller disappears. An item never
observed has `vendors: null` and `reason: "item not in dataset"`; known empty is
`vendors: []`, `reason: null`; sellers present is a nonempty list. Human absence
wording remains “no known sellers in the vendor index,” never a universal claim.

## Selection and credentials

A nonblank `EVERLEDGER_FOREVERDB_API_KEY` wins over the file selected by
`EVERLEDGER_FOREVERDB_API_KEY_FILE`, which defaults to
`EVERLEDGER_DATA_DIR/foreverdb-api-key` (`~/.everledger/foreverdb-api-key` normally).
Absent/empty keys leave ForeverDB unavailable. No credential is stored in an
index or printed; upstream error bodies and transport messages are suppressed.

Ninja remains active until a key and a complete ForeverDB index both exist.
ForeverDB then activates automatically. During a failed refresh, its previous
index may serve stale for up to 7 days for transport/server failures; HTTP 4xx
and older indexes select ninja fallback. Ninja's own refresh failures retain
its existing 7-day stale-cache rule. A successful ninja fallback after a
ForeverDB failure is marked stale to disclose the failure (exit 3). No qualifying
fallback exits with a source error. Normal cached answers older than one day
remain stale until refresh, matching the original cache behavior. Removing the
key immediately restores ninja. The backend directories and provenance remain
separate; no item answer combines rows from the two sources.

Every ForeverDB answer, including `price` and `status`, includes
“Data from ForeverDB.net (https://foreverdb.net)” in its source metadata/line.
Status reports the active backend plus ForeverDB configuration, last successful
sync, vendor/item record counts, pending state, and last attempt/count.

## Offline verification

Run `python3 -m unittest discover -s tests` from the repository. Tests cover
sync/index construction; the four Ironforge Red Dye sellers; independent vendor
and sell provenance; unknown labels; all three absence states; cross-zone
multiple locations; listed copper prices and attribution in `vendors`/`price`;
AHledger-only margins; incremental change/removal; daily budgets and pacing;
checkpoint resumption after a mid-detail interrupt; abrupt-exit cooldown;
recovery after an extraction changes during pagination; honest pending reporting; atomic preservation of the
old index; a failed index remaining ineligible through an unfinished retry; malformed mapping; credential precedence and redaction (including invalid header exceptions); keyless
and pre-sync dormancy; stale-index and ninja fallback; and the real CLI refresh
path with fixture transport. ForeverDB requests are replaced in-process; other
CLI integrations use a local fixture server.

## Live acceptance on key day — required before trusting the adapter

1. From the repo, run the offline suite above. Keep the current ninja cache.
   Put the newly approved key in the default file using a local editor; do not
   paste it into terminal commands or logs:

   ```bash
   export EVERLEDGER_DATA_DIR="${EVERLEDGER_DATA_DIR:-$HOME/.everledger}"
   umask 077
   mkdir -p "$EVERLEDGER_DATA_DIR"
   "${EDITOR:-vi}" "$EVERLEDGER_DATA_DIR/foreverdb-api-key"
   chmod 600 "$EVERLEDGER_DATA_DIR/foreverdb-api-key"
   ```

2. `./everledger status --json` must show ninja active and ForeverDB awaiting a
   completed sync (assuming no previous completed ForeverDB index). This makes
   no ForeverDB request. For the first live attempt, inspect the mapping boundary
   with the debugger (this runs the same paced, budgeted refresh):

   ```bash
   python3 -m pdb ./everledger refresh catalogue --json
   ```

   At the debugger prompt, set `b everledger_foreverdb_mapping.py:46` and
   `b everledger_foreverdb_mapping.py:66`, then `c`. At each mapping breakpoint
   use `pp payload` to inspect the response, then `c` to continue. Do not inspect
   transport locals or request headers. These line numbers are for this version;
   after editing, use the first executable line in `page` and `vendor`.
   Verify **every provisional
   assumption above**, including revision coverage with ForeverDB's documented
   guarantees or an observed changed sell. Unknown revision semantics are not
   acceptance. If the shape differs, keep ninja active, correct only the mapping
   and corresponding fixtures, rerun the suite, and wait for the next daily
   attempt. Do not bypass the cooldown or increase the budget.

3. Check `vendor_sync.requests <= 36`. If pending, run the same refresh command
   after at least 24 hours from the prior attempt's end, repeating until complete.
   Sum actual counts and compare with P + N (plus any failed/repeated requests).
   Confirm status keeps ninja active until completion, then names foreverdb.net,
   last sync time and the actual record counts. If bootstrap spans extraction
   changes, verify the completed list/detail records describe a compatible
   extraction before accepting them. An extraction mismatch during pagination
   automatically discards the obsolete checkpoint and waits for the next daily
   attempt to restart; the active index remains intact.

4. Run these commands and inspect the human output and JSON:

   ```bash
   ./everledger vendors 2604 --in IF
   ./everledger vendors 2604 --in IF --json
   ./everledger price 2604
   ./everledger status --json
   ```

   Confirm Poranna Snowbraid, Outfitter Eric, Burbik Gearspanner and Bombus
   Finespindle against actual API records. Check every returned location and
   each sale's copper price/label without inferring faction or quantity. Confirm
   backend attribution plus the credit/link on every output using ForeverDB.
   Verify a real unknown item, a locally known item whose sellers disappeared,
   and an item with sellers retain the three distinct JSON states and bounded
   human wording. Use fixture coverage for a state absent from the live dataset.

5. Run `./everledger refresh catalogue --json` again immediately: ForeverDB
   requests must be zero. After the next eligible 24 hours run it once more:
   expect P requests if unchanged, P + C when revisions changed. Check removals,
   changed prices and labels are reflected and multi-location data survives.
   An absent sell-change revision guarantee leaves acceptance incomplete.
   Finish with the offline interrupt/fallback tests; do not deliberately generate
   production API errors to prove failure handling.

## Learnings

Vendor sightings and observed sales are separate claims; only the sell record's
own label can strengthen sale/price provenance. A detail-per-vendor bootstrap
cannot honestly promise a few dozen total requests, so its daily budget and
multi-day completion must be explicit. The API's actual shape and revision
semantics remain unverified; fixture success is preparation for acceptance,
not evidence that the real API contract has been confirmed.
