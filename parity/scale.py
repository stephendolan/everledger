#!/usr/bin/env python3
"""Parity and timing at production size: a ForeverDB index of 1,014 vendors.

Usage: python3 parity/scale.py CANDIDATE

The replayed corpus and the fuzz use small datasets. A real index is tens of
megabytes, and a checkpoint late in a sync is as large. This builds both with
the oracle's own mapping and snapshot code, then compares and times:

- lookups and status against the complete index;
- the final attempt of a sync, which fetches the last four details from a
  fixture and publishes the index;
- a whole sync in one attempt, which a daily cap above 1,025 allows: 1,025
  requests and a checkpoint rewritten after each, growing to full size. Each
  side runs it once, and the reference takes several minutes.

CANDIDATE needs the `parity-seams` feature. Build it optimized, or the final
attempt outlasts the engine's process timeout:

    cargo build --release --features parity-seams --target-dir target/seams
    python3 parity/scale.py target/seams/release/everledger
"""
import datetime
import json
import os
import random
import sys
import tempfile
import time
import types
from pathlib import Path

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.engine import Engine, Normalizer, ORIGIN, ROOT, compare, snapshot
from parity.fixture import Fixture

VENDORS = 1014
REMAINING = 4
MARKET = 'forever.pvp.alliance.us'
PAGES = ['alchemy', 'blacksmithing', 'enchanting', 'engineering', 'leatherworking', 'tailoring',
         'cooking', 'first-aid', 'fishing', 'mining', 'herbalism', 'skinning']


def dataset():
    """The service's responses for a full dataset: list pages and vendor details."""
    rng = random.Random(5)
    zones = ['Ironforge', 'Stormwind City', 'Orgrimmar', 'Thunder Bluff', 'Darnassus', 'Undercity',
             'The Barrens', 'Dun Morogh', 'Elwynn Forest']
    items = rng.sample(range(100, 40000), 3000)
    ids = rng.sample(range(1, 90000), VENDORS)
    details = {}
    for vendor in ids:
        sells = []
        for item in rng.sample(items, rng.randrange(3, 40)):
            stack = rng.choice([1, 1, 1, 5, 20])
            sells.append({'itemId': item, 'name': f'Item {item}',
                          'price': rng.choice([None, stack * rng.randrange(1, 9000), 10]), 'stack': stack,
                          'limited': rng.random() < 0.2, 'limitedStockCount': rng.choice([None, 1, 5]),
                          'seenInForever': rng.choice([True, False, None]), 'classicData': rng.choice([True, False]),
                          'pricePerItem': rng.uniform(0, 90)})
        details[str(vendor)] = {'data': {
            'id': vendor, 'name': f'Vendor {vendor}', 'zone': rng.choice(zones), 'mapId': rng.randrange(1, 2000),
            'x': round(rng.uniform(0, 100), 1), 'y': round(rng.uniform(0, 100), 1),
            'side': rng.choice(['Alliance', 'Horde', 'Both']), 'title': 'Trade Supplies',
            'seenInForever': True, 'classicData': True, 'sells': sells}}
    pages = [{'data': [{'id': n} for n in ids[start:start + 100]],
              'meta': {'build': '1.60.1.70938', 'total': VENDORS, 'limit': 100, 'offset': start,
                       'next': start + 100 if start + 100 < VENDORS else None}}
             for start in range(0, VENDORS, 100)]
    return {'pages': pages, 'details': details}, [str(n) for n in ids]


def oracle_state(forever, ids):
    """index.json for a finished sync and pending.json for one four details short, as Python writes them."""
    sys.path.insert(0, str(ROOT))
    import everledger_core as core
    import everledger_foreverdb as db
    import everledger_foreverdb_mapping as mapping
    origin = datetime.datetime.fromisoformat(ORIGIN.replace('Z', '+00:00'))
    stamp = lambda **ago: (origin - datetime.timedelta(**ago)).isoformat(timespec='seconds').replace('+00:00', 'Z')
    db.now = core.now = lambda: origin - datetime.timedelta(hours=1)
    mapped = {key: {'data': mapping.vendor(forever['details'][key], int(key))} for key in ids}
    metadata = {'build': '1.60.1.70938', 'vendor_total': VENDORS, 'published_at': None}
    dump = lambda value: json.dumps(value, ensure_ascii=False).encode()
    finished = {'format': 2, 'sync_id': 'a' * 32, 'details': mapped, 'metadata': metadata, 'total_requests': 1025}
    index = db.snapshot(finished, None)
    index['sync_id'] = finished['sync_id']
    busiest = max(index['items'], key=lambda key: len(index['items'][key]['vendors']))
    complete = {
        'vendors/foreverdb.net/index.json': dump(index),
        'vendors/foreverdb.net/attempt.json': dump({
            'attempted_at': stamp(hours=1, minutes=2), 'requests': 17, 'sync_id': 'a' * 32, 'failed': False,
            'status': None, 'total_requests': 1025, 'last_request_at': stamp(hours=1, minutes=1),
            'finished_at': stamp(hours=1)}),
    }
    pending = {'format': 2, 'sync_id': 'b' * 32, 'started_at': stamp(days=29), 'next_offset': None,
               'vendor_ids': ids, 'details': {key: mapped[key] for key in ids[:-REMAINING]},
               'metadata': metadata, 'total_requests': 1025 - REMAINING}
    final_attempt = {
        'vendors/foreverdb.net/pending.json': dump(pending),
        'vendors/foreverdb.net/attempt.json': dump({
            'attempted_at': stamp(days=1, hours=2), 'requests': 36, 'sync_id': 'b' * 32, 'failed': False,
            'status': None, 'total_requests': 1025 - REMAINING, 'last_request_at': stamp(days=1, hours=1, minutes=59),
            'finished_at': stamp(days=1, hours=1, minutes=58)}),
    }
    return complete, final_attempt, busiest, list(index['items'])[:2]


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    candidate = [str(Path(sys.argv[1]).resolve())]
    forever, ids = dataset()
    complete, final_attempt, busiest, sample = oracle_state(forever, ids)
    catalogue = (ROOT / 'tests/fixtures/catalogue-fixture.json').read_bytes()
    routes = {'/': (200, ''), '/characters': (200, {'characters': [], 'sources': []}),
              '/listings': (200, {'listings': [], 'sources': []}),
              '/events': (200, {'events': [], 'next_cursor': None, 'checkpoint': 'terminal', 'has_more': False, 'sources': []}),
              f'/v1/pricetable/{MARKET}': (200, 'market|version|%d|2' % datetime.datetime.fromisoformat(ORIGIN.replace('Z', '+00:00')).timestamp())}
    for page in PAGES:
        routes[f'/professions/{page}/'] = (200, 'Recipes (0)')
    fixture = Fixture(types.SimpleNamespace(routes=routes, requests=[], event_contract=None))
    fixture.forever = forever
    engine = Engine(candidate)
    oracle = [sys.executable, str(ROOT / 'parity/oracle.py')]
    env = {k: v for k, v in os.environ.items() if not k.startswith('EVERLEDGER_')}
    env.update(TZ='UTC', EVERLEDGER_ROSTER_TOKEN='fixture', EVERLEDGER_FOREVERDB_API_KEY='secret-fixture')
    timings = []

    def scenario(name, files, invocations):
        with tempfile.TemporaryDirectory(prefix='everledger-scale-') as directory, \
                tempfile.TemporaryDirectory(prefix='everledger-scale-controls-') as controls:
            for path, raw in {'catalogue.json': catalogue, **files}.items():
                target = Path(directory, path)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(raw)
            initial = snapshot(directory)
            engine.start_scenario()
            for index, argv in enumerate(invocations):
                row = [' '.join(argv)]
                for side in (oracle, candidate):
                    started = time.perf_counter()
                    engine.capture(side, argv, env, fixture, directory, initial, Path(controls))
                    row.append(time.perf_counter() - started)
                timings.append(row)
                # capture() leaves the last run's files behind; step() snapshots the directory first.
                for path, raw in initial.items():
                    if not path.endswith('/'):
                        Path(directory, path).write_bytes(raw)
                for stale in set(snapshot(directory)) - set(initial):
                    if not stale.endswith('/'):
                        Path(directory, stale).unlink()
                engine.step(f'scale/{name}/{index}', argv, env, fixture, directory, forbidden=('secret-fixture',))

    def single_attempt():
        argv = ['refresh', 'catalogue', '--json']
        raised = dict(env, EVERLEDGER_FOREVERDB_DAILY_REQUESTS='5000')
        with tempfile.TemporaryDirectory(prefix='everledger-scale-') as directory, \
                tempfile.TemporaryDirectory(prefix='everledger-scale-controls-') as controls:
            Path(directory, 'catalogue.json').write_bytes(catalogue)
            initial = snapshot(directory)
            row, sides = [' '.join(argv) + ' (whole sync)'], []
            engine.timeout = 3600
            try:
                for side in (oracle, candidate):
                    started = time.perf_counter()
                    sides.append(engine.capture(side, argv, raised, fixture, directory, initial, Path(controls)))
                    row.append(time.perf_counter() - started)
            finally:
                engine.timeout = 30
            timings.append(row)
            state, differences = compare(Normalizer().transcript(sides[0]), Normalizer().transcript(sides[1]))
            engine.records.append({'name': 'scale/single-attempt/0/replay', 'command': 'refresh', 'state': state,
                                   'differences': differences, 'oracle': sides[0], 'candidate': sides[1]})
            # Equal transcripts prove nothing if both sides stopped early.
            sent = sum(request['path'].startswith('/api/v1/') for request in sides[0]['requests'])
            if sent != len(forever['pages']) + VENDORS or 'vendors/foreverdb.net/index.json' not in sides[0]['files']:
                engine.errors.append(f'scale/single-attempt: the reference sent {sent} requests and did not finish the sync')
            for side, transcript in zip(('oracle', 'candidate'), sides):
                if any(b'secret-fixture' in raw for raw in [transcript['stdout'], transcript['stderr'], *transcript['files'].values()]):
                    engine.errors.append(f'scale/single-attempt: {side} leaked sentinel')

    try:
        scenario('complete-index', complete, [
            ['vendors', busiest, '--json'], ['vendors', busiest, '--in', 'IF'], ['vendors', sample[0]],
            ['vendors', '999999', '--json'], ['status', '--json']])
        scenario('final-attempt', final_attempt, [['refresh', 'catalogue', '--json']])
        single_attempt()
    finally:
        fixture.close()
        engine.close()
    print(f'{"invocation":<40} {"oracle s":>9} {"candidate s":>12} {"ratio":>7}')
    for label, a, b in timings:
        print(f'{label:<40} {a:9.2f} {b:12.2f} {b / a:7.2f}')
    states = [row['state'] for row in engine.records]
    for row in engine.records:
        if row['state'] == 'unexplained':
            fields = ', '.join(d['field'] for d in row['differences'] if d['class'] is None)
            print(f"{row['name']}: {' '.join(row['oracle']['argv'])} [{fields}]")
    for error in engine.errors:
        print(error, file=sys.stderr)
    print(f'steps {len(states)}, identical {states.count("identical")}, sanctioned {states.count("sanctioned")}, '
          f'unexplained {states.count("unexplained")}, harness failures {len(engine.errors)}')
    return 1 if 'unexplained' in states or engine.errors else 0


if __name__ == '__main__':
    sys.exit(main())
