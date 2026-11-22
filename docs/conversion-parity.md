# Conversion parity

`everledger parity` joins the local catalogue to the freshest AHledger price
table on each run. Every recipe with a positive quantity, a priced result, and
prices for all reagents can appear. Each item needs a floor and a median in the
same scan. A row's input and output quantities describe one conversion. JSON
lists them along with its catalogue profession and spell ID or item-use URL.

Essence conversion is a bag item use. The [item-use records](../data/item-use-conversions.json)
link to the Forever client tooltip for the consumed item in each direction.
Those records supply five pairs, each with a three-to-one combine and a
one-to-three split. Keep the tooltip link beside each ratio so a client change
can be checked at the source.

## Price bases

| JSON field | Input cost | Output value |
| --- | --- | --- |
| `floor` | Reagents at scan floors | Output at scan floor |
| `median` | Reagents at scan medians | Output at scan median |
| `floor_realized` | Reagents at scan floors | Output floor minus 1c per unit, at least 1c, after the 5% AH cut |
| `median_after_cut` | Reagents at scan medians | Output at scan median after the 5% AH cut |

All copper totals apply the catalogue or item-use quantities. Spread is output
value less input cost. The denominator for spread percent is input cost.
The verdict compares `floor_realized` and `median_after_cut`, so the two
sale scenarios both account for the cut. The CLI uses the realized floor
spread as its headline. A floor or median opportunity stays visible in the
default view. Request `--all` to include conversions that break even or lose
copper on both bases.

The ranking divides realized spread by output quantity before multiplying by
AHledger's estimated output units sold per day. The result is a whole-market
comparison in copper per day. It omits other costs such as expired listing
deposits and any opportunity cost of a cooldown. Listed units and sales/day
come from the same table. Unknown sales estimates stay null in JSON and rank
after known estimates.

The source summary gives the AHledger scan time and age. Stale scans still
print and exit 3. Each invocation fetches a price table and computes the rows
in memory.

## Learnings

- Use the catalogue's reagent quantities at the price join. On the
  2026-11-22 03:33:31Z scan, Bolt of Woolen Cloth takes three Wool Cloth and
  its realized spread is +51c (+14.2%). The Magic Essence split returns
  three Lesser Magic Essence and realizes +69c (+20.9%).
- Compare net proceeds for a sale verdict. Ruined Leather Scraps into
  Light Leather gained 2c at the gross median on the reference scan and
  lost 2c after the cut. The floor-realized spread was -14c.
- A price table replay needs the scan time, two price fields, listed units,
  sales estimate and the catalogue revision. Keep regression fixtures
  synthetic and replay historical inputs locally through the AHledger URL
  override when reproducing research.
