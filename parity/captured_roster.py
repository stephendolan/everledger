#!/usr/bin/env python3
"""Roster responses in the captured shapes, through the public commands of both implementations.

Usage: python3 parity/captured_roster.py CANDIDATE [--write]

`tests/fixtures/captured-roster.json` holds the world: responses with the keys and JSON types the
roster service sent on November 19, 2026, and the commands to run over them. Each command runs in
both renderings through the reference and the candidate, which must exit 0 with identical stdout.

`tests/golden/captured-roster.json` records what the reference did with each command: its exit
code, stdout, and requests. `tests/read_paths.rs` replays that file in-process, so `cargo test`
holds the binary to the reference on these shapes without running Python. This script fails when
the file no longer matches the reference; `--write` regenerates it.
"""
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

WORLD = ROOT / 'tests/fixtures/captured-roster.json'
GOLDEN = ROOT / 'tests/golden/captured-roster.json'


def run(candidate, write=False):
    world = json.loads(WORLD.read_text())
    routes = {path: (200, body) for path, body in world['routes'].items()}
    fixture = Fixture(types.SimpleNamespace(routes=routes, requests=[], event_contract=None))
    observed = []
    try:
        with tempfile.TemporaryDirectory() as directory:
            shutil.copyfile(ROOT / 'tests/fixtures/catalogue-fixture.json', Path(directory) / 'catalogue.json')
            env = {k: v for k, v in os.environ.items() if not k.startswith('EVERLEDGER_')}
            env.update(TZ='UTC', EVERLEDGER_DATA_DIR=directory, EVERLEDGER_ROSTER_URL=fixture.base,
                       EVERLEDGER_ROSTER_TOKEN='fixture', EVERLEDGER_AHLEDGER_URL=fixture.base,
                       EVERLEDGER_VENDOR_URL=fixture.base, EVERLEDGER_SEAM_FOREVERDB_URL=fixture.base,
                       EVERLEDGER_SEAM_NOW=world['now'], EVERLEDGER_SEAM_SLEEP_LOG=str(Path(directory) / 'sleep.log'),
                       EVERLEDGER_PARITY_ELAPSED=str(Path(directory) / 'elapsed'),
                       HTTP_PROXY='http://127.0.0.1:9', HTTPS_PROXY='http://127.0.0.1:9',
                       NO_PROXY='127.0.0.1,localhost')
            Path(env['EVERLEDGER_PARITY_ELAPSED']).write_text('0.0')
            for command in world['commands']:
                for mode in ([], ['--json']):
                    args = [*command, *mode]
                    fixture.reset()
                    oracle = subprocess.run([str(ROOT / 'parity/oracle-candidate'), *args], env=env, capture_output=True)
                    requests = [row['path'] for row in fixture.log]
                    fixture.reset()
                    answer = subprocess.run([str(candidate), *args], env=env, capture_output=True)
                    assert oracle.returncode == answer.returncode == 0, (args, oracle.stderr, answer.stderr)
                    assert oracle.stdout == answer.stdout, (args, oracle.stdout, answer.stdout)
                    assert requests == [row['path'] for row in fixture.log], (args, requests, fixture.log)
                    observed.append({'argv': args, 'exit_code': oracle.returncode,
                                     'stdout': oracle.stdout.decode(), 'requests': requests})
                    if args == ['listings', '--json']:
                        listing = world['routes']['/listings']['listings'][0]
                        recorded = listing['price']['recorded']
                        expected = dict(listing, **recorded, available=listing['price']['available'],
                                        floor_copper=87, undercut=True)
                        assert json.dumps(json.loads(answer.stdout)['listings'][0]) == json.dumps(expected)
                    if args == ['events', '--json']:
                        assert json.loads(answer.stdout)['entries'] == world['routes']['/events']['entries']
    finally:
        fixture.close()
    recorded = json.dumps(observed, indent=1, ensure_ascii=False) + '\n'
    if write:
        GOLDEN.write_text(recorded)
    assert GOLDEN.read_text() == recorded, f'{GOLDEN.relative_to(ROOT)} is not what the reference prints; rerun with --write'
    print(f'captured roster: {len(observed)} stdout byte comparisons; both exit codes 0; '
          'listing key order preserved; golden matches the reference')


if __name__ == '__main__':
    run(Path(sys.argv[1]).resolve(), write='--write' in sys.argv[2:])
