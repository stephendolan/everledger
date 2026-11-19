#!/usr/bin/env python3
"""Compare captured roster shapes through the public commands, using loopback only."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import types

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from parity.fixture import Fixture


def run(candidate):
    stamp = '2026-11-15T12:00:00Z'
    source = {'available': True, 'file': 'DataStore.lua', 'globals': {}, 'mtime': stamp,
              'mtime_ns': 1794744000000000000, 'size_bytes': 100}
    character = {'class': 'Mage', 'class_id': 8, 'faction': 'Alliance', 'gold_copper': 100,
                 'hearth': {'location': 'Ironforge'}, 'level': 60, 'name': 'Slade Undertow',
                 'partial_data': [], 'professions': [{'max_skill': 75, 'name': 'Tailoring',
                 'recipes': {}, 'skill': 5, 'skill_available': True}],
                 'professions_updated_at': stamp, 'race': 'Human', 'race_id': 1, 'realm': 'Forever',
                 'short_name': 'Slade', 'sources': {k: 'DataStore.lua' for k in
                 ('gold_copper', 'hearth', 'level', 'professions')}}
    listing = {'auctionID': 123, 'character': 'Slade Undertow', 'count': 2,
               'expirationTime': 1794747600, 'item_id': 2604, 'location': {}, 'name': 'Red Dye',
               'price': {'available': True, 'recorded': {'buyout_copper': 200, 'bid_copper': 100,
               'has_bid': False, 'deposit_copper': 10, 'expires_at': stamp}},
               'realm': 'Forever', 'source': 'DataStore_Auctions', 'undercut': None}
    entries = []
    for index, kind in enumerate(['purchase', 'sale', 'posting', 'expiration'] * 2):
        entries.append({'actor': {}, 'barter_items': [] if index % 2 else None,
                        'cash_basis': 'recorded', 'cash_copper': 90, 'character': 'Slade Undertow',
                        'cost_basis_eligible': True, 'count': 1, 'counterparty': 'Other' if index % 2 else None,
                        'currencies': [] if index % 2 else None, 'deposit_copper': 1 if index % 2 else None,
                        'fee_copper': None, 'id': str(index), 'item': {'id': 2604, 'name': 'Red Dye'},
                        'item_match_basis': None, 'payment_confirmed': None, 'posting_basis': 'recorded',
                        'price_basis': 'recorded', 'proceeds_copper': None, 'provenance': {}, 'quote': {},
                        'raw': {}, 'recorded_at': stamp, 'type': kind,
                        'unit_copper': [1.5, 1, None][index % 3], 'value_copper': 100 if index % 2 else None,
                        'venue': 'auction'})
    page = {'available': True, 'checkpoint': 'terminal', 'complete': True, 'coverage': {},
            'entries': entries, 'filters': {'actor': None, 'item': None, 'since': 1794657600,
            'types': [], 'until': None, 'venue': None}, 'generated_at': stamp, 'has_more': False,
            'item_matching': {'basis': None, 'unresolved_linkless_count': 0, 'unresolved_recorded_names': []},
            'matched_count': 8, 'mode': 'snapshot', 'next_cursor': None, 'reason': None, 'returned_count': 8,
            'roster_enrichment': {}, 'schema_version': 1, 'snapshot_at': stamp, 'snapshot_id': 'snapshot',
            'source_id': 'journal', 'state': 'available'}
    routes = {'/characters/Slade': (200, {'generated_at': stamp, 'sources': [source], 'character': character}),
              '/characters': (200, {'characters': [{'name': 'Slade Undertow'}], 'sources': [source]}),
              '/characters/Slade%20Undertow/inventory': (200, {'sources': [source],
              'bags': {'available': True, 'items': [{'item_id': 2604, 'name': 'Red Dye', 'count': 3}]}}),
              '/listings': (200, {'filters': {}, 'generated_at': stamp, 'listings': [listing], 'returned_count': 1,
              'scans': [{'available': True, 'character': 'Slade Undertow', 'pricing': {}}],
              'sources': [dict(source, file_age_seconds=0, max_age_seconds=900, stale=False)]}),
              '/events': (200, page),
              '/v1/pricetable/forever.pvp.alliance.us': (200, 'm|v|1794744000\n2604:88:87:709:::::10')}
    vocabulary = types.SimpleNamespace(routes=routes, requests=[], event_contract=None)
    fixture = Fixture(vocabulary)
    try:
        with tempfile.TemporaryDirectory() as directory:
            shutil.copyfile(ROOT / 'tests/fixtures/catalogue-fixture.json', Path(directory) / 'catalogue.json')
            env = {k: v for k, v in os.environ.items() if not k.startswith('EVERLEDGER_')}
            env.update(TZ='UTC', EVERLEDGER_DATA_DIR=directory, EVERLEDGER_ROSTER_URL=fixture.base,
                       EVERLEDGER_ROSTER_TOKEN='fixture', EVERLEDGER_AHLEDGER_URL=fixture.base,
                       EVERLEDGER_VENDOR_URL=fixture.base, EVERLEDGER_SEAM_FOREVERDB_URL=fixture.base,
                       EVERLEDGER_SEAM_NOW=stamp, EVERLEDGER_PARITY_ELAPSED=str(Path(directory) / 'elapsed'), HTTP_PROXY='http://127.0.0.1:9', HTTPS_PROXY='http://127.0.0.1:9',
                       NO_PROXY='127.0.0.1,localhost')
            env['EVERLEDGER_SEAM_SLEEP_LOG'] = str(Path(directory) / 'sleep.log')
            Path(env['EVERLEDGER_PARITY_ELAPSED']).write_text('0.0')
            commands = [('char', 'Slade'), ('events',), ('events', '--limit', '0'), ('sales',),
                        ('purchases',), ('expirations',), ('listings',), ('net', '2604'), ('positions',)]
            for command in commands:
                for mode in [(), ('--json',)]:
                    args = (*command, *mode)
                    oracle = subprocess.run([str(ROOT / 'parity/oracle-candidate'), *args], env=env, capture_output=True)
                    candidate_result = subprocess.run([str(candidate), *args], env=env, capture_output=True)
                    assert oracle.returncode == candidate_result.returncode == 0, (args, oracle.stderr, candidate_result.stderr)
                    assert oracle.stdout == candidate_result.stdout, (args, oracle.stdout, candidate_result.stdout)
                    if args == ('listings', '--json'):
                        result = json.loads(candidate_result.stdout)
                        expected = dict(listing, buyout_copper=200, bid_copper=100, has_bid=False,
                                        deposit_copper=10, expires_at=stamp, available=True, floor_copper=87, undercut=True)
                        assert json.dumps(result['listings'][0]) == json.dumps(expected)
                    if args == ('events', '--json'):
                        assert json.loads(candidate_result.stdout)['entries'] == entries
            print('captured roster: 18 stdout byte comparisons; both exit codes 0; listing key order preserved')
    finally:
        fixture.close()


if __name__ == '__main__':
    run(Path(sys.argv[1]).resolve())
