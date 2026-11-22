# Planning margins

```sh
everledger margins [PROFESSION] [--level N] [--char NAME] [--json]
```

Both formats return the full set within the profession, level, and known-recipe
scope. Where skill is known, priced recipes sort by skill-up color first. Within
a color, positive planning margins are ranked by estimated daily sales ×
margin, and losses by margin. Unavailable rows follow priced rows.

Rule v2 uses the current floor for the sell quote when listed units exceed
twice the unrounded estimated daily sales rate. At or below the boundary it
uses the current median. The lower of this quote and the 7-day median is the
planning sell price. Multiply by output yield, apply the 5% auction cut, and
round proceeds to the nearest copper with ties to even. Subtract the basket
cost from the same scan's current reagent floors.

In JSON, `margin_copper` is the planning margin per craft, `sells_copper` is
the planning sell price per unit, and `cost_copper` is the current reagent
cost per craft. `price_basis` identifies the binding sell price
(`current_floor`, `current_median`, or `7d_median`). Each reagent's
`unit_copper` is its current AHledger floor. An unavailable result has a null
margin and an `unavailable_reason`. A valid zero reagent floor is a price.

To verify a live capture, archive the exact AHledger pricetable response and
catalogue, replay that response to both CLIs through `EVERLEDGER_AHLEDGER_URL`,
and save unfiltered `margins --json` output from each. Run the independent
recomputation and coverage check from the repository root.

```sh
python3 scripts/check_planning_margins.py catalogue.json pricetable.txt rust.json python.json
```

The pricetable's scan timestamp belongs in the report. The feed may revise
prices under the same timestamp, so retain the response bytes or their hash
when reconciling calculations. Differential parity alone can pass when both
implementations use the same wrong basis. The basis-sensitive fixture asserts
worked v2 amounts before comparing Rust with Python.
