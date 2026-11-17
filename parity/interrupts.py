#!/usr/bin/env python3
"""Interrupts and retry delays the differential runner cannot stage.

Usage: python3 parity/interrupts.py CANDIDATE

The runner interrupts a sync once, with stderr open, and compares what is left.
This holds a process at chosen points and checks both implementations:

- **A dead stderr.** SIGINT at a held ForeverDB request, after the reader of
  stderr has gone. Each side must end by SIGINT within seconds with its
  journal closed, so that the sync lock is free for the next refresh.
- **After the sync.** SIGINT while the ninja refresh that follows an incomplete
  sync waits on a response. Each side must end by SIGINT.
- **Retry delays.** A retryable 502 whose `retry_after` runs from seconds to
  numbers no date can hold. Both sides must send the same requests and exit
  with the same code. Where the reference dies of an overflow its journal
  stays open, and the candidate's is closed and marked failed: the one
  sanctioned difference here.

CANDIDATE needs the `parity-seams` feature.
"""
import json
import os
import signal
import subprocess
import sys
import tempfile
from pathlib import Path

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.corpus import DBScenario, load_cli
from parity.engine import Engine, Normalizer, ROOT, restore, snapshot

ORACLE = [sys.executable, str(ROOT / 'parity/oracle.py')]
JOURNAL = 'vendors/foreverdb.net/attempt.json'
REFRESH = ['refresh', 'catalogue', '--json']
# From an ordinary delay, through the last a date can hold, to integers and floats beyond one.
DELAYS = ['75', '90.5', '-5', '"soon"', 'true', 'null', '1e11', '3e11', '1e13', '1e14', '1e20',
          '100000000000000000000', '1e300', '1e999', '-1e999', '1' + '0' * 309, '-1' + '0' * 309]
WAIT = 10
failures = []
sanctioned = []


def check(condition, message):
    print(('ok    ' if condition else 'FAIL  ') + message)
    if not condition:
        failures.append(message)


def foreverdb(log):
    return [row['path'] for row in log if row['path'].startswith('/api/v1/')]


def spawn(engine, scenario, command):
    controls = tempfile.mkdtemp(prefix='everledger-interrupts-')
    Path(controls, 'elapsed').write_text('0.0')
    Path(controls, 'sleeps').write_bytes(b'')
    env = engine.environment(scenario.env, scenario.fixture, scenario.root, Path(controls))
    return subprocess.Popen([*command, *REFRESH], cwd=ROOT, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, start_new_session=True)


def ending(process, scenario):
    """How the process ended within WAIT seconds; None if the check had to kill it."""
    try:
        process.wait(WAIT)
        return process.returncode
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()
        return None
    finally:
        scenario.fixture.release.set()


def dead_stderr(engine, module, name, command):
    scenario = DBScenario(engine, module, 'interrupts-dead-stderr')
    try:
        scenario.fixture.scripts = {3: {'hold': True}}
        scenario.fixture.reset()
        process = spawn(engine, scenario, command)
        if not scenario.fixture.held.wait(20):
            ending(process, scenario)
            return check(False, f'{name} reaches its held third ForeverDB request')
        process.stderr.close()
        os.killpg(process.pid, signal.SIGINT)
        code = ending(process, scenario)
        check(code == -signal.SIGINT, f'{name} ends by SIGINT when nothing reads its stderr (ended: {code})')
        journal = json.loads((scenario.root / JOURNAL).read_text())
        check(bool(journal.get('finished_at')) and journal.get('failed') is True and journal.get('requests') == 3,
              f'{name} closes its journal first: three requests, finished, failed')
        check(len(journal.get('spends', [])) == 1 and journal['spends'][0]['requests'] == 3,
              f'{name} records the interrupted attempt as one spend of three requests')
    finally:
        scenario.close()


def after_the_sync(engine, module, name, command):
    scenario = DBScenario(engine, module, 'interrupts-after-sync')
    try:
        # A ninja cache file gives the refresh something to fetch once the sync has ended incomplete.
        with tempfile.TemporaryDirectory() as controls:
            engine.capture(command, ['vendors', '2604', '--json'], scenario.env, scenario.fixture, scenario.root,
                           snapshot(scenario.root), Path(controls))
        state = snapshot(scenario.root)
        scenario.fixture.large()
        answer = scenario.fixture.response

        def hold_ninja(path):
            status, payload, headers, held = answer(path)
            return status, payload, headers, held or path.startswith('/items/')
        scenario.fixture.response = hold_ninja
        restore(scenario.root, state)
        scenario.fixture.reset()
        process = spawn(engine, scenario, command)
        if not scenario.fixture.held.wait(30):
            ending(process, scenario)
            return check(False, f'{name} reaches the ninja refresh after its sync')
        spent = len(foreverdb(scenario.fixture.log))
        os.killpg(process.pid, signal.SIGINT)
        code = ending(process, scenario)
        check(spent == 36, f'{name} has ended its sync at the cap before the signal ({spent} requests)')
        check(code == -signal.SIGINT, f'{name} ends by SIGINT after its sync, during the ninja refresh (ended: {code})')
    finally:
        scenario.close()


def retry_delays(engine, module, candidate):
    scenario = DBScenario(engine, module, 'interrupts-retry-delays')
    try:
        state = snapshot(scenario.root)
        for value in DELAYS:
            shown = value if len(value) < 24 else f'{value[:6]}... ({len(value)} characters)'
            scenario.fixture.scripts = {1: {'status': 502, 'body': '{"retryable": true, "retry_after": %s}' % value}}
            runs = []
            for command in (ORACLE, candidate):
                with tempfile.TemporaryDirectory() as controls:
                    runs.append(engine.capture(command, REFRESH, scenario.env, scenario.fixture, scenario.root,
                                               state, Path(controls)))
            reference, port = runs
            check(foreverdb(reference['requests']) == foreverdb(port['requests']) and reference['sleep_log'] == port['sleep_log'],
                  f'retry_after {shown}: the same requests at the same times '
                  f'({len(foreverdb(reference["requests"]))} and {len(foreverdb(port["requests"]))})')
            check(reference['exit_code'] == port['exit_code'] and reference['stdout'] == port['stdout'],
                  f'retry_after {shown}: the same exit code and stdout ({reference["exit_code"]} and {port["exit_code"]})')
            if Normalizer().transcript(reference)['files'] == Normalizer().transcript(port)['files']:
                check(True, f'retry_after {shown}: the same journal and files')
            elif b'OverflowError' in reference['stderr']:
                # The reference died mid-attempt with its journal still open. The port's must be closed.
                journals = [json.loads(run['files'][JOURNAL]) for run in runs]
                closed = bool(journals[1].get('finished_at')) and journals[1].get('failed') is True
                check('finished_at' not in journals[0] and closed and journals[0]['requests'] == journals[1]['requests'],
                      f'retry_after {shown}: the reference dies with its journal open; the port closes its own as failed')
                sanctioned.append(shown)
            else:
                check(False, f'retry_after {shown}: the same journal and files')
    finally:
        scenario.close()


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    candidate = [str(Path(sys.argv[1]).resolve())]
    engine = Engine(candidate, both=False)
    module = load_cli()
    try:
        for name, command in (('Python', ORACLE), ('Rust', candidate)):
            dead_stderr(engine, module, name, command)
            after_the_sync(engine, module, name, command)
        retry_delays(engine, module, candidate)
    finally:
        engine.close()
    print(f'sanctioned {len(sanctioned)}: the reference dies of an overflow on retry_after ' + ', '.join(sanctioned))
    print(f'{len(failures)} failures')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
