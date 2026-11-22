#!/usr/bin/env python3
"""Verify unfiltered margins JSON against the exact AHledger response and catalogue."""
import argparse
from collections import Counter, defaultdict, deque
from datetime import datetime, timezone
from decimal import Decimal
from fractions import Fraction
import json
from pathlib import Path


def read_scan(path):
    header, *lines = path.read_text().splitlines()
    _, market, timestamp, *_ = header.split('|')
    prices = {}
    for line in lines:
        item, median, floor, supply, history, _, _, _, rate = line.split(':')
        prices[int(item)] = {
            'median': round(Decimal(median)) if median else None,
            'floor': round(Decimal(floor)) if floor else None,
            'supply': int(Decimal(supply)) if supply else None,
            'history': round(Decimal(history)) if history else None,
            'rate': Fraction(rate) / 10 if rate and Decimal(rate).is_finite() else None,
        }
    return market.replace('/', '.'), datetime.fromtimestamp(int(timestamp), timezone.utc), prices


def expected(recipe, prices, ambiguous):
    units = []
    for reagent in recipe['reagents']:
        item = reagent['item_id']
        floor = prices.get(item, {}).get('floor') if item and item > 0 else None
        units.append(floor if floor is not None and floor >= 0 and not ambiguous else None)
    result = dict(margin_copper=None, cost_copper=None, sells_copper=None,
                  price_basis=None, sales_per_day=None, unavailable_reason=None,
                  reagent_units=units)
    if ambiguous:
        result['unavailable_reason'] = 'ambiguous_recipe_mapping'
        return result

    if None not in units:
        result['cost_copper'] = sum(unit * reagent['qty'] for unit, reagent in zip(units, recipe['reagents']))
    output = recipe['result']
    item = output['item_id']
    scan = prices.get(item) if item and item > 0 else None
    rate = scan['rate'] if scan else None
    result['sales_per_day'] = float(rate) if rate is not None else None
    if not item or item < 0:
        reason = 'missing_output_mapping'
    elif output['qty'] <= 0:
        reason = 'invalid_output_yield'
    elif scan is None:
        reason = 'missing_output_price'
    elif rate is None or rate <= 0:
        reason = 'missing_positive_sales_rate'
    else:
        supply = scan['supply']
        basis = 'current_median'
        quote = None
        if supply is not None and supply >= 0:
            if Fraction(supply, 2) > rate:
                quote, basis = scan['floor'], 'current_floor'
            else:
                quote = scan['median']
        history = scan['history']
        if not quote or quote < 0:
            reason = 'missing_current_quote'
        elif not history or history < 0:
            reason = 'missing_7d_median'
        else:
            sell = min(history, quote)
            result['sells_copper'] = sell
            result['price_basis'] = '7d_median' if history < quote else basis
            cost = result['cost_copper']
            if cost is not None:
                result['margin_copper'] = round(Fraction(19 * sell * output['qty'], 20)) - cost
                return result
            reason = ('missing_reagent_mapping'
                      if any((r['item_id'] or 0) <= 0 for r in recipe['reagents'])
                      else 'missing_reagent_price')
    result['unavailable_reason'] = reason
    return result


def check(catalogue_path, scan_path, report_paths):
    market, stamp, prices = read_scan(scan_path)
    catalogue = json.loads(catalogue_path.read_text())
    recipes = [(prof, recipe) for prof, block in catalogue['professions'].items()
               for recipe in block['recipes']]
    spell_counts = Counter(r['spell_id'] for _, r in recipes)
    expected_rows = defaultdict(list)
    for prof, recipe in recipes:
        spell = recipe['spell_id']
        expected_rows[prof, spell].append(expected(recipe, prices, spell_counts[spell] > 1))
    success = True
    for path in report_paths:
        payload = json.loads(path.read_text())
        pending = {key: deque(rows) for key, rows in expected_rows.items()}
        errors = []
        sources = [s for s in payload['sources'] if s['name'] == f'AHledger {market}']
        if len(sources) != 1 or datetime.fromisoformat(sources[0]['updated_at']) != stamp:
            errors.append(f'expected source AHledger {market} at {stamp.isoformat()}')
        counts = Counter()
        for row in payload['margins']:
            key = (row['profession'], row['spell_id'])
            if not pending.get(key):
                errors.append(f'unexpected or duplicate recipe row {key}')
                continue
            want = pending[key].popleft()
            actual = {field: row[field] for field in want if field != 'reagent_units'}
            actual['reagent_units'] = [r['unit_copper'] for r in row['reagents']]
            for field, value in want.items():
                if actual[field] != value:
                    errors.append(f'{key} {row["name"]} {field}: expected {value}, got {actual[field]}')
            counts[want['unavailable_reason'] or want['price_basis']] += 1
        missing = [key for key, rows in pending.items() if rows]
        if missing:
            errors.append(f'missing rows for {len(missing)} recipe keys (first five: {missing[:5]})')
        if errors:
            success = False
            print(f'{path}: {len(errors)} errors')
            for error in errors[:20]:
                print(f'  {error}')
        else:
            print(f'{path}: {len(recipes)} rows match {market} at {stamp.isoformat()}')
            print('  ' + ', '.join(f'{key}={value}' for key, value in sorted(counts.items())))
    return success


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('catalogue', type=Path)
    parser.add_argument('pricetable', type=Path)
    parser.add_argument('margins_json', type=Path, nargs='+')
    args = parser.parse_args()
    raise SystemExit(0 if check(args.catalogue, args.pricetable, args.margins_json) else 1)
