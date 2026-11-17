#!/usr/bin/env python3
"""Both implementations on one data directory: taking over a sync, and the sync lock.

Usage: python3 parity/handoff.py CANDIDATE

During cutover the Python reference and the Rust binary read and write the same
data directory. The differential runner gives each side its own copy of the
state, so it never runs one implementation on files the other wrote, and never
runs both at once. This does:

- **Takeover.** A 40-vendor sync needs two daily attempts. One implementation
  makes the first and the other finishes it, in both orders. The published
  index, the journal, and the finishing attempt's output must equal those of
  the reference making both attempts. The two attempts run at the default
  daily cap, and again with a different cap each, as two wrappers with
  different environments would.
- **Lock.** One implementation holds the sync lock mid-request while the other
  starts a refresh on the same directory. The second must wait, send no
  ForeverDB request, and then defer to the cooldown, in both orders.

CANDIDATE needs the `parity-seams` feature.
"""
import json
import os
import subprocess
import sys
import tempfile
import time
import types
from pathlib import Path

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.engine import Engine, Normalizer, ROOT, snapshot
from parity.fixture import Fixture

PAGES = ['alchemy', 'blacksmithing', 'enchanting', 'engineering', 'leatherworking', 'tailoring',
         'cooking', 'first-aid', 'fishing', 'mining', 'herbalism', 'skinning']
ORACLE = [sys.executable, str(ROOT / 'parity/oracle.py')]
SYNC_FILES = ('vendors/foreverdb.net/index.json', 'vendors/foreverdb.net/attempt.json')
failures = []


def check(condition, message):
    print(('ok    ' if condition else 'FAIL  ') + message)
    if not condition:
        failures.append(message)


def fixture():
    routes = {f'/professions/{page}/': (200, 'Recipes (0)') for page in PAGES}
    return Fixture(types.SimpleNamespace(routes=routes, requests=[], event_contract=None))


def environment(cap=None):
    env = {k: v for k, v in os.environ.items() if not k.startswith('EVERLEDGER_')}
    env.update(TZ='UTC', EVERLEDGER_ROSTER_TOKEN='fixture', EVERLEDGER_FOREVERDB_API_KEY='secret-fixture')
    if cap is not None:
        env['EVERLEDGER_FOREVERDB_DAILY_REQUESTS'] = cap
    return env


def a_day_passes(directory):
    path = Path(directory, 'vendors/foreverdb.net/attempt.json')
    state = json.loads(path.read_text())
    state.update(attempted_at='2020-01-01T00:00:00Z', finished_at='2020-01-01T00:00:00Z')
    path.write_text(json.dumps(state))


def takeover(engine, first, second, caps):
    """Two attempts on one directory, each by the named side under its own cap; the finishing attempt's observables."""
    server = fixture()
    server.large()
    try:
        with tempfile.TemporaryDirectory(prefix='everledger-handoff-') as directory, \
                tempfile.TemporaryDirectory(prefix='everledger-handoff-controls-') as controls:
            argv = ['refresh', 'catalogue', '--json']
            opening = engine.capture(first, argv, environment(caps[0]), server, directory, snapshot(directory), Path(controls))
            a_day_passes(directory)
            closing = engine.capture(second, argv, environment(caps[1]), server, directory, snapshot(directory), Path(controls))
            return opening, Normalizer().transcript(closing)
    finally:
        server.close()


def lock(engine, holder, waiter, holder_name, waiter_name):
    server = fixture()
    # Request three is the second vendor's detail; hold it with its real body.
    second = str(server.forever['pages'][0]['data'][1]['id'])
    server.scripts = {3: {'hold': True, 'body': server.forever['details'][second]}}
    try:
        with tempfile.TemporaryDirectory(prefix='everledger-handoff-') as directory, \
                tempfile.TemporaryDirectory(prefix='everledger-handoff-a-') as first_controls, \
                tempfile.TemporaryDirectory(prefix='everledger-handoff-b-') as second_controls:
            argv = ['refresh', 'catalogue', '--json']
            processes = []
            for command, controls in ((holder, first_controls), (waiter, second_controls)):
                Path(controls, 'elapsed').write_text('0.0')
                Path(controls, 'sleeps').write_bytes(b'')
                env = engine.environment(environment(), server, directory, Path(controls))
                processes.append(subprocess.Popen([*command, *argv], cwd=ROOT, env=env, stdout=subprocess.PIPE,
                                                  stderr=subprocess.PIPE))
                if len(processes) == 1 and not server.held.wait(20):
                    processes[0].kill()
                    check(False, f'{holder_name} never reached its held ForeverDB request')
                    return
            time.sleep(3)  # long enough for the waiter to build the catalogue and reach the lock
            sync_requests = lambda: [row['path'] for row in server.log if row['path'].startswith('/api/v1/')]
            check(processes[1].poll() is None, f'{waiter_name} waits while {holder_name} holds the sync lock')
            check(len(sync_requests()) == 3, f'{waiter_name} sends no ForeverDB request while it waits')
            server.release.set()
            outputs = [process.communicate(timeout=60) for process in processes]
            check(processes[0].returncode == 0 and len(sync_requests()) == 6,
                  f'{holder_name} finishes its six requests and nothing more reaches the service')
            try:
                deferred = json.loads(outputs[1][0])['vendor_sync']
            except (ValueError, KeyError):
                deferred = {}
            check(deferred.get('deferred') == 'daily sync limit' and deferred.get('requests') == 0,
                  f'{waiter_name} then defers to the cooldown with zero requests')
    finally:
        server.release.set()
        server.close()


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    candidate = [str(Path(sys.argv[1]).resolve())]
    engine = Engine(candidate)
    try:
        # The cap of each attempt, and the requests the first one then spends on 40 vendors.
        for caps, spent in (((None, None), 36), ((None, '5000'), 36), (('7', None), 7)):
            _, reference = takeover(engine, ORACLE, ORACLE, caps)
            for first, second, order in ((ORACLE, candidate, 'Rust finishes a sync Python started'),
                                         (candidate, ORACLE, 'Python finishes a sync Rust started')):
                label = f'{order} (caps {caps[0] or "default"}, then {caps[1] or "default"})'
                opening, closing = takeover(engine, first, second, caps)
                requests = sum(row['path'].startswith('/api/v1/') for row in opening['requests'])
                check(requests == spent, f'{label}: the first attempt spends its {spent}-request cap')
                for name in SYNC_FILES:
                    check(closing['files'].get(name) == reference['files'].get(name) and name in closing['files'],
                          f'{label}: {name.rsplit("/", 1)[1]} equals the all-Python result')
                check('vendors/foreverdb.net/pending.json' not in closing['files'], f'{label}: the checkpoint is gone')
                for field in ('exit_code', 'stdout', 'requests', 'sleep_log'):
                    check(closing[field] == reference[field], f'{label}: finishing attempt has the same {field}')
        lock(engine, ORACLE, candidate, 'Python', 'Rust')
        lock(engine, candidate, ORACLE, 'Rust', 'Python')
    finally:
        engine.close()
    print(f'{len(failures)} failures')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
