#!/usr/bin/env python3
"""Both implementations on one data directory: taking over a sync, and the sync lock.

Usage: python3 parity/handoff.py CANDIDATE

During cutover the Python reference and the Rust binary read and write the same
data directory. The differential runner gives each side its own copy of the
state, so it never runs one implementation on files the other wrote, and never
runs both at once. This does:

- **Takeover.** A 40-vendor sync takes two attempts. One implementation makes
  the first and the other finishes it, in both orders. The published index,
  the journal with its spends, and the finishing attempt's output must equal
  those of the reference making both attempts. The attempts run a day apart
  at the default cap, and back to back under a different cap each, as two
  wrappers with different environments would: the second attempt then spends
  what the first left of its own cap. The implementation that did not publish
  the index then answers a vendor lookup from it.
- **An index from before the blocks were dropped.** An index written by an
  older build carries `details` and `sync_requests`, which nothing reads. Both
  implementations must answer from such an index whatever those hold, and sync
  again over it.
- **Files an earlier build wrote.** Cutover hands the port a data directory
  that earlier builds of the reference filled. Each build this port has been
  pinned to completes a small sync and starts a 40-vendor one, and the first
  build to cache a ninja lookup caches one. The current reference and the
  candidate then read each directory: lookups, status, and a refresh that
  resumes or replaces what is there. They must give the same answers and leave
  the same files, whether that is carrying on or, for a journal from before
  spends were recorded, refusing. The builds come out of this clone's history.
- **Lock.** One implementation holds the sync lock mid-request while the other
  starts a refresh on the same directory, both at a cap of 6. The second must
  wait, send no ForeverDB request, and then find the window spent, in both
  orders.

CANDIDATE needs the `parity-seams` feature.
"""
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import types
from pathlib import Path

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.corpus import DBScenario, load_cli
from parity.engine import Engine, Normalizer, ROOT, elapsed, snapshot
from parity.oracle import ORACLE_FILES
from parity.fixture import Fixture

PAGES = ['alchemy', 'blacksmithing', 'enchanting', 'engineering', 'leatherworking', 'tailoring',
         'cooking', 'first-aid', 'fishing', 'mining', 'herbalism', 'skinning']
ORACLE = [sys.executable, str(ROOT / 'parity/oracle.py')]
SYNC_FILES = ('vendors/foreverdb.net/index.json', 'vendors/foreverdb.net/attempt.json')
REFRESH = ['refresh', 'catalogue', '--json']
LOOKUPS = (['vendors', '2604', '--json'], ['vendors', '2604'])
STATUS = ['status', '--json']
# Each commit of the reference this port has been pinned to before the present one, oldest first.
EARLIER = ('32f05ab', 'b24e183', 'd28214c', '6f72b44')
# The first build whose ForeverDB module the clock shim can load; ninja cache files have kept their shape since.
FIRST_CACHE = '4b3d1c2'
KEY = 'EVERLEDGER_FOREVERDB_API_KEY'
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


def takeover(engine, first, second, caps, gap):
    """Two attempts on one directory, gap seconds apart, each by the named side under its own cap.

    Returns the first attempt's transcript, the finishing attempt's observables, and what the
    first side then answers from the index the second published.
    """
    server = fixture()
    server.large()
    try:
        with tempfile.TemporaryDirectory(prefix='everledger-handoff-') as directory, \
                tempfile.TemporaryDirectory(prefix='everledger-handoff-controls-') as controls:
            argv = ['refresh', 'catalogue', '--json']
            engine.start_scenario()
            opening = engine.capture(first, argv, environment(caps[0]), server, directory, snapshot(directory), Path(controls))
            engine.advance(seconds=elapsed(opening) + gap)
            closing = engine.capture(second, argv, environment(caps[1]), server, directory, snapshot(directory), Path(controls))
            engine.advance(seconds=elapsed(closing))
            published = snapshot(directory)
            answers = [engine.capture(first, lookup, environment(caps[1]), server, directory, published, Path(controls))
                       for lookup in LOOKUPS]
            return opening, Normalizer().transcript(closing), [(answer['exit_code'], answer['stdout']) for answer in answers]
    finally:
        server.close()


def legacy_blocks(engine, candidate):
    """Both sides read an index that still carries `details` and `sync_requests`, then sync again over it."""
    server = fixture()
    try:
        with tempfile.TemporaryDirectory(prefix='everledger-handoff-') as directory, \
                tempfile.TemporaryDirectory(prefix='everledger-handoff-controls-') as controls:
            engine.start_scenario()
            synced = engine.capture(ORACLE, REFRESH, environment(), server, directory, snapshot(directory), Path(controls))
            engine.advance(seconds=elapsed(synced))
            path = Path(directory, SYNC_FILES[0])
            index = json.loads(path.read_text())
            path.write_text(json.dumps({**index, 'details': {'obsolete': 'ignored'}, 'sync_requests': -1}, ensure_ascii=False))
            trimmed = snapshot(directory)
            for argv in (*LOOKUPS, REFRESH):
                reference, port = (engine.capture(side, argv, environment(), server, directory, trimmed, Path(controls))
                                   for side in (ORACLE, candidate))
                same = all(reference[field] == port[field] for field in ('exit_code', 'stdout', 'requests', 'sleep_log'))
                check(same and reference['exit_code'] == 0,
                      f'an index still carrying details and sync_requests: {" ".join(argv)} gives the same answer '
                      f'(exit {reference["exit_code"]} and {port["exit_code"]})')
    finally:
        server.close()


def earlier_build(commit, directory):
    """The reference as it stood at `commit`, unpacked with today's clock shim; None if this clone lacks the commit."""
    archive = subprocess.run(['git', '-C', str(ROOT), 'archive', commit], capture_output=True)
    if archive.returncode:
        return None
    tree = Path(directory, commit)
    with tarfile.open(fileobj=io.BytesIO(archive.stdout)) as files:
        files.extractall(tree, filter='data')
    (tree / 'python').mkdir()
    for name in ORACLE_FILES:
        (tree / name).rename(tree / 'python' / name)
    (tree / 'parity').mkdir()
    shutil.copy(ROOT / 'parity/oracle.py', tree / 'parity/oracle.py')
    shutil.copytree(ROOT / 'parity/oracle_hook', tree / 'parity/oracle_hook')
    return [sys.executable, str(tree / 'parity/oracle.py')]


def agree(engine, scenario, candidate, label, state, commands):
    """The current reference and the candidate from one state: the same answer, and the same files left."""
    pairs = []
    for argv in commands:
        with tempfile.TemporaryDirectory() as controls:
            reference, port = (engine.capture(side, argv, scenario.env, scenario.fixture, scenario.root, state, Path(controls))
                               for side in (ORACLE, candidate))
        same = all(reference[field] == port[field] for field in ('exit_code', 'stdout', 'requests', 'sleep_log'))
        same = same and Normalizer().transcript(reference)['files'] == Normalizer().transcript(port)['files']
        check(same, f'{label}: {" ".join(argv)} gives the same answer and files (exit {reference["exit_code"]} and {port["exit_code"]})')
        pairs.append((reference, port))
    return pairs


def written(engine, scenario, writer, argv, state, label):
    """What an earlier build leaves after one command, or None with the check failed."""
    with tempfile.TemporaryDirectory() as controls:
        attempt = engine.capture(writer, argv, scenario.env, scenario.fixture, scenario.root, state, Path(controls))
    check(attempt['exit_code'] == 0, f'{label}: the earlier build runs {" ".join(argv)} (exit {attempt["exit_code"]})')
    engine.advance(seconds=elapsed(attempt))
    return attempt['files'] if attempt['exit_code'] == 0 else None


def earlier_sync(engine, module, candidate, commit, writer, large):
    """An earlier build syncs: five vendors to the end, or forty as far as a day's cap goes. A day later the current sides read on."""
    scenario = DBScenario(engine, module, f'handoff-{commit}-{"started" if large else "complete"}')
    try:
        if large:
            scenario.fixture.large()
        engine.start_scenario()
        what = f'a sync {commit} {"started" if large else "completed"}'
        state = written(engine, scenario, writer, REFRESH, snapshot(scenario.root), what)
        if state is None:
            return
        engine.advance(days=1)
        what += ', a day later'
        (reference, port), = agree(engine, scenario, candidate, what, state, (*LOOKUPS, STATUS, REFRESH))[-1:]
        engine.advance(seconds=max(elapsed(reference), elapsed(port)))
        for side, files in (('the reference', reference['files']), ('the candidate', port['files'])):
            agree(engine, scenario, candidate, f'{what}, after {side} refreshed over it', files, LOOKUPS)
    finally:
        scenario.close()


def earlier_cache(engine, module, candidate, commit, writer):
    """An earlier build caches a ninja lookup of a sold item and of an unknown one. Two days later the current sides read on."""
    scenario = DBScenario(engine, module, f'handoff-{commit}-cache')
    del scenario.env[KEY]
    try:
        engine.start_scenario()
        what = f'ninja cache files {commit} wrote'
        state = snapshot(scenario.root)
        for item in ('2604', '999999'):
            state = written(engine, scenario, writer, ['vendors', item, '--json'], state, what)
            if state is None:
                return
        engine.advance(days=2)
        agree(engine, scenario, candidate, what + ', two days later', state,
              (*LOOKUPS, ['vendors', '999999', '--json'], STATUS, REFRESH))
    finally:
        scenario.close()


def earlier_builds(engine, candidate):
    module = load_cli()
    with tempfile.TemporaryDirectory(prefix='everledger-handoff-builds-') as directory:
        for commit in (FIRST_CACHE, *EARLIER):
            writer = earlier_build(commit, directory)
            check(writer is not None, f'the reference at {commit} comes out of this clone')
            if writer is None:
                continue
            earlier_cache(engine, module, candidate, commit, writer)
            if commit in EARLIER:
                earlier_sync(engine, module, candidate, commit, writer, large=False)
                earlier_sync(engine, module, candidate, commit, writer, large=True)


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
            engine.start_scenario()
            processes = []
            for command, controls in ((holder, first_controls), (waiter, second_controls)):
                Path(controls, 'elapsed').write_text('0.0')
                Path(controls, 'sleeps').write_bytes(b'')
                # At a cap of 6 the holder's sync spends the whole window.
                env = engine.environment(environment('6'), server, directory, Path(controls))
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
                  f'{waiter_name} then finds the window spent and sends nothing')
    finally:
        server.release.set()
        server.close()


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    candidate = [str(Path(sys.argv[1]).resolve())]
    engine = Engine(candidate)
    try:
        # Each attempt's cap, the seconds between the two, and what the first then spends on 40 vendors.
        for caps, gap, spent in (((None, None), 86400, 36), ((None, '5000'), 0, 36), (('7', '50'), 0, 7)):
            _, reference, reference_answers = takeover(engine, ORACLE, ORACLE, caps, gap)
            for first, second, order in ((ORACLE, candidate, 'Rust finishes a sync Python started'),
                                         (candidate, ORACLE, 'Python finishes a sync Rust started')):
                label = (f'{order} (caps {caps[0] or "default"} then {caps[1] or "default"}, '
                         f'{"a day apart" if gap else "back to back"})')
                opening, closing, answers = takeover(engine, first, second, caps, gap)
                requests = sum(row['path'].startswith('/api/v1/') for row in opening['requests'])
                check(requests == spent, f'{label}: the first attempt spends its {spent}-request cap')
                for name in SYNC_FILES:
                    check(closing['files'].get(name) == reference['files'].get(name) and name in closing['files'],
                          f'{label}: {name.rsplit("/", 1)[1]} equals the all-Python result')
                check('vendors/foreverdb.net/pending.json' not in closing['files'], f'{label}: the checkpoint is gone')
                for field in ('exit_code', 'stdout', 'requests', 'sleep_log'):
                    check(closing[field] == reference[field], f'{label}: finishing attempt has the same {field}')
                reader = 'Python' if first is ORACLE else 'Rust'
                check(answers == reference_answers and all(code == 0 for code, _ in answers),
                      f'{label}: {reader} then answers a vendor lookup from the index the other published')
        legacy_blocks(engine, candidate)
        earlier_builds(engine, candidate)
        lock(engine, ORACLE, candidate, 'Python', 'Rust')
        lock(engine, candidate, ORACLE, 'Rust', 'Python')
    finally:
        engine.close()
    print(f'{len(failures)} failures')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
