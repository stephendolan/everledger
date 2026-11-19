#!/usr/bin/env python3
"""Compare unread character and inventory values against a baseline binary."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import types

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from parity.fixture import Fixture


def run(baseline, candidate):
    values = [('plain', '{}'), *[(f'depth-{n}', '[' * n + ']' * n) for n in (100, 126, 127, 128, 200)],
              ('nan', 'NaN'), ('infinity', '[Infinity]'), ('big-int', '123456789012345678901234567890'),
              ('big-float', '1e400'), ('neg-zero', '-0'), ('duplicate', '{"a":1,"a":2}')]
    vocabulary = types.SimpleNamespace(routes={}, requests=[], event_contract=None)
    fixture = Fixture(vocabulary)
    different = unexpected = total = 0
    try:
        with tempfile.TemporaryDirectory() as directory:
            Path(directory, 'catalogue.json').write_bytes((ROOT / 'tests/fixtures/catalogue-fixture.json').read_bytes())
            env = {'PATH': os.environ['PATH'], 'HOME': directory, 'TZ': 'UTC',
                   'EVERLEDGER_DATA_DIR': directory, 'EVERLEDGER_ROSTER_URL': fixture.base,
                   'EVERLEDGER_ROSTER_TOKEN': 'fixture', 'EVERLEDGER_AHLEDGER_URL': fixture.base,
                   'EVERLEDGER_VENDOR_URL': fixture.base, 'EVERLEDGER_SEAM_FOREVERDB_URL': fixture.base,
                   'EVERLEDGER_SEAM_NOW': '2026-11-19T12:30:00Z',
                   'EVERLEDGER_SEAM_SLEEP_LOG': str(Path(directory, 'sleeps')),
                   'HTTP_PROXY': 'http://127.0.0.1:9', 'HTTPS_PROXY': 'http://127.0.0.1:9',
                   'NO_PROXY': '127.0.0.1,localhost'}
            for label, value in values:
                for location in ('character-page', 'character-row', 'selector-page', 'selector-row',
                                 'inventory-page', 'inventory-section', 'inventory-row'):
                    routes = {'/characters': (200, {'characters': [{'name': 'Slade'}]}),
                              '/characters/Slade': (200, {'character': {'name': 'Slade'}}),
                              '/characters/Slade/inventory': (200, {'bags': {'available': True, 'items': []}}),
                              '/events': (200, {'entries': [], 'has_more': False, 'checkpoint': 'cp'}),
                              '/listings': (200, {'listings': []}),
                              '/v1/pricetable/forever.pvp.alliance.us': (200, 'market|1|1795263900\n2604:500:450:709:::::3808')}
                    if location.startswith(('character', 'selector')):
                        row = '{"name":"Slade","professions":[]' + (',"extra":' + value if location.endswith('row') else '') + '}'
                        body = '{"character":' + row + (',"extra":' + value if location.endswith('page') else '') + '}'
                        routes['/characters/Slade'] = (200, body)
                        argv = ['char', 'Slade', '--json'] if location.startswith('character') else ['events', '--char', 'Slade', '--since', '30d', '--json']
                    else:
                        row = '{"item_id":2604,"name":"Red Dye","count":1' + (',"extra":' + value if location.endswith('row') else '') + '}'
                        section = '{"available":true,"items":[' + row + ']' + (',"extra":' + value if location.endswith('section') else '') + '}'
                        body = '{"bags":' + section + (',"extra":' + value if location.endswith('page') else '') + '}'
                        routes['/characters/Slade/inventory'] = (200, body)
                        argv = ['positions', '--json']
                    vocabulary.routes = routes
                    observations = []
                    for binary in (baseline, candidate):
                        fixture.reset()
                        result = subprocess.run([str(binary), *argv], env=env, capture_output=True, timeout=60)
                        observations.append((result.returncode, result.stdout, result.stderr,
                                             [r['path'] for r in fixture.log]))
                    total += 1
                    if observations[0] != observations[1]:
                        different += 1
                        a, b = observations
                        depth_change = label in ('depth-127', 'depth-128', 'depth-200') and a[0] == 1 and b[0] == 0 and b'recursion limit exceeded' in a[2] and not a[1] and not b[2]
                        numeric_change = label in ('nan', 'infinity', 'big-float') and a[0] == b[0] == 1 and not a[1] and not b[1] and a[2] in (b'roster: invalid response shape (expected ident at line 1 column 2)\n', b'roster: invalid response shape (expected value at line 1 column 1)\n') and b[2] == b'roster: invalid response shape (expected value)\n' and a[3] == b[3]
                        sanctioned = location == 'selector-page' and (depth_change or numeric_change)
                        unexpected += not sanctioned
                        if not sanctioned:
                            print('  baseline:', repr(a[2]), 'candidate:', repr(b[2]))
                        print(f'{"sanctioned R3" if sanctioned else "UNEXPECTED"} {location}/{label}: '
                              f'exit {observations[0][0]} -> {observations[1][0]}')
            print(f'{total} character/inventory cases, {different} differ, {unexpected} unexpected')
    finally:
        fixture.close()
    return bool(unexpected)


if __name__ == '__main__':
    sys.exit(run(Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()))
