#!/usr/bin/env python3
"""Cache files no sync writes: what each implementation does with a damaged one.

Usage: python3 parity/damage.py CANDIDATE

A sync writes its checkpoint, journal, and index itself, so the corpus and the
fuzz only ever hand an implementation files it wrote. This puts other things
in their place: the wrong JSON type, a missing key, a number where text
belongs, bytes that are not UTF-8. For each damaged file both implementations
run the commands that read it, from the same state, and must exit with the
same code, send the same requests, and print the same stdout.

Where the reference carries on with a payload it could not have written and
the candidate refuses it, the case is named in `SANCTIONED` with its reason. A
candidate may refuse only by exiting 1 without a ForeverDB request. One that
sends a request the reference does not is never sanctioned.

CANDIDATE needs the `parity-seams` feature.
"""
import copy
import json
import sys
import tempfile
from pathlib import Path

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.corpus import CAP, DBScenario, load_cli
from parity.engine import Engine, ROOT, elapsed, snapshot

ORACLE = [sys.executable, str(ROOT / 'parity/oracle.py')]
CACHE = 'vendors/foreverdb.net/'
REFRESH = ['refresh', 'catalogue', '--json']
STATUS = ['status', '--json']
LOOKUP = ['vendors', '2604', '--json']
NOT_JSON = b'{not json'
NOT_UTF8 = b'\xff'
# The reference computes with whatever a spend holds; the port needs whole, non-negative counts in a list.
SPENDS = "the journal's spends are not a list of whole, non-negative request counts"
SANCTIONED = {(prefix + label, 'refresh'): SPENDS
              for prefix in ('journal: ', 'journal inside the window: ')
              for label in ('spends is an object', 'a spend of 1.5 requests', 'a spend of -1 requests',
                            'a spend of true requests')}
counts = {'identical': 0, 'sanctioned': 0}
failures = []


def raw(value):
    return value if isinstance(value, bytes) else json.dumps(value).encode()


def run(engine, scenario, command, argv, state):
    with tempfile.TemporaryDirectory() as controls:
        return engine.capture(command, argv, scenario.env, scenario.fixture, scenario.root, state, Path(controls))


def compare(engine, scenario, candidate, label, state, commands):
    for argv in commands:
        reference, port = (run(engine, scenario, side, argv, state) for side in (ORACLE, candidate))
        paths = [[row['path'] for row in side['requests']] for side in (reference, port)]
        same = (reference['exit_code'] == port['exit_code'] and paths[0] == paths[1]
                and reference['stdout'] == port['stdout'])
        if same:
            counts['identical'] += 1
            continue
        sent = [sum(path.startswith('/api/v1/') for path in side) for side in paths]
        reason = SANCTIONED.get((label, argv[0]))
        refused = port['exit_code'] == 1 and sent[1] == 0 and not port['stdout']
        summary = (f'{label}: {argv[0]}: exit {reference["exit_code"]} and {port["exit_code"]}, '
                   f'ForeverDB requests {sent[0]} and {sent[1]}, other requests {len(paths[0]) - sent[0]} and {len(paths[1]) - sent[1]}')
        if reason and refused:
            counts['sanctioned'] += 1
            print(f'sanctioned  {summary} ({reason})')
        else:
            failures.append(summary)
            print(f'FAIL        {summary}')


def with_file(state, name, content):
    changed = dict(state)
    if content is None:
        changed.pop(name, None)
    else:
        changed[name] = raw(content)
    return changed


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    candidate = [str(Path(sys.argv[1]).resolve())]
    engine = Engine(candidate, both=False)
    module = load_cli()
    scenario = DBScenario(engine, module, 'damage')
    try:
        fresh = snapshot(scenario.root)

        def reach(cap, state, days=1):
            """The reference's files after one refresh under a cap, and the clock moved on."""
            if cap is None:
                scenario.env.pop(CAP, None)
            else:
                scenario.env[CAP] = cap
            attempt = run(engine, scenario, ORACLE, REFRESH, state)
            scenario.env.pop(CAP, None)
            engine.advance(seconds=elapsed(attempt), days=days)
            return attempt['files']

        engine.start_scenario()
        synced = reach(None, fresh)                 # a complete index, and a whole budget again
        index = json.loads(synced[CACHE + 'index.json'])
        journal = json.loads(synced[CACHE + 'attempt.json'])
        stamp = journal['finished_at']

        for label, content in [('an empty object', {}), ('an empty list', []), ('a list', [1]), ('a string', 'x'),
                               ('a number', 5), ('zero', 0), ('true', True), ('false', False), ('null', None),
                               ('an object of another format', {'format': 1}), ('a format with nothing else', {'format': 2}),
                               ('not UTF-8', NOT_UTF8), ('not JSON', NOT_JSON)]:
            compare(engine, scenario, candidate, 'checkpoint: ' + label,
                    with_file(synced, CACHE + 'pending.json', raw(content) if content is not None else b'null'),
                    [REFRESH, STATUS])

        def spoil_index(**changes):
            return {**index, **changes}
        without_sync_id = {key: value for key, value in index.items() if key != 'sync_id'}
        for label, content in [('a list', [1]), ('a string', 'x'), ('a number', 5), ('true', True), ('an empty object', {}),
                               ('an empty list', []), ('null', b'null'), ('not UTF-8', NOT_UTF8), ('not JSON', NOT_JSON),
                               ('format 2.0', spoil_index(format=2.0)), ('format as text', spoil_index(format='2')),
                               ('one record too many counted', spoil_index(record_count=index['record_count'] + 1)),
                               ('one item too many counted', spoil_index(item_count=index['item_count'] + 1)),
                               ('no sync_id', without_sync_id), ('items is a list', spoil_index(items=[])),
                               ('fetched_at is not a time', spoil_index(fetched_at='soon'))]:
            compare(engine, scenario, candidate, 'index: ' + label, with_file(synced, CACHE + 'index.json', content),
                    [REFRESH, STATUS, LOOKUP])

        def spoil_journal(*drop, **changes):
            changed = {key: value for key, value in copy.deepcopy(journal).items() if key not in drop}
            changed.update(changes)
            return changed
        spend = lambda **fields: [{'requests': 6, 'finished_at': stamp, **fields}]
        for label, content in [
                ('spends is null', spoil_journal(spends=None)), ('spends is an object', spoil_journal(spends={})),
                ('spends holds a number', spoil_journal(spends=[1])), ('spends holds an empty object', spoil_journal(spends=[{}])),
                ('a spend without requests', spoil_journal(spends=[{'finished_at': stamp}])),
                ('a spend without finished_at', spoil_journal(spends=[{'requests': 1}])),
                ('a spend finished at null', spoil_journal(spends=spend(finished_at=None))),
                ('a spend finished at no time', spoil_journal(spends=spend(finished_at='soon'))),
                ('a spend of "1" requests', spoil_journal(spends=spend(requests='1'))),
                ('a spend of 1.5 requests', spoil_journal(spends=spend(requests=1.5))),
                ('a spend of -1 requests', spoil_journal(spends=spend(requests=-1))),
                ('a spend of true requests', spoil_journal(spends=spend(requests=True))),
                ('a spend with another key', spoil_journal(spends=spend(note='x'))),
                ('a spend in the future', spoil_journal(spends=spend(finished_at='2030-01-01T00:00:00Z'))),
                ('no spends', spoil_journal('spends')), ('no spends and unfinished', spoil_journal('spends', 'finished_at')),
                ('unfinished', spoil_journal('finished_at')), ('unfinished without a last request', spoil_journal('finished_at', 'last_request_at')),
                ('unfinished without any time', spoil_journal('finished_at', 'last_request_at', 'attempted_at')),
                ('unfinished without requests', spoil_journal('finished_at', 'requests')),
                ('failed is 1', spoil_journal(failed=1)), ('status is text', spoil_journal(failed=True, status='503')),
                ('an empty list', []), ('a list', [1]), ('null', b'null'), ('an empty object', {}),
                ('not UTF-8', NOT_UTF8), ('not JSON', NOT_JSON)]:
            compare(engine, scenario, candidate, 'journal: ' + label, with_file(synced, CACHE + 'attempt.json', content),
                    [REFRESH, STATUS, LOOKUP])

        engine.start_scenario()
        recent = reach(None, fresh, days=0)         # the same journal with its spend still inside the window
        live = json.loads(recent[CACHE + 'attempt.json'])
        counted = lambda **fields: [{'requests': 6, 'finished_at': live['finished_at'], **fields}]
        for label, spends in [('spends is an object', {}), ('spends holds a number', [1]),
                              ('a spend without requests', [{'finished_at': live['finished_at']}]),
                              ('a spend of "1" requests', counted(requests='1')), ('a spend of 1.5 requests', counted(requests=1.5)),
                              ('a spend of -1 requests', counted(requests=-1)), ('a spend of true requests', counted(requests=True)),
                              ('a spend of 40 requests', counted(requests=40)), ('a spend in the future', counted(finished_at='2030-01-01T00:00:00Z'))]:
            compare(engine, scenario, candidate, 'journal inside the window: ' + label,
                    with_file(recent, CACHE + 'attempt.json', {**live, 'spends': spends}), [REFRESH, STATUS, LOOKUP])

        engine.start_scenario()
        midway = reach('2', fresh)                  # a checkpoint one detail in, and a whole budget again
        checkpoint = json.loads(midway[CACHE + 'pending.json'])
        for label, content in [('format 2.0', {**checkpoint, 'format': 2.0}), ('format as text', {**checkpoint, 'format': '2'}),
                               ('a retry at no time', {**checkpoint, 'retry': {'not_before': 'soon'}}),
                               ('a retry at null', {**checkpoint, 'retry': {'not_before': None}}),
                               ('an empty retry', {**checkpoint, 'retry': {}}), ('a retry that is text', {**checkpoint, 'retry': 'x'}),
                               ('no sync_id', {key: value for key, value in checkpoint.items() if key != 'sync_id'}),
                               ('next_offset as text', {**checkpoint, 'next_offset': '0'}), ('details is null', {**checkpoint, 'details': None}),
                               ('total_requests as text', {**checkpoint, 'total_requests': 'x'}),
                               ('vendor_ids holds a number', {**checkpoint, 'vendor_ids': [1]})]:
            compare(engine, scenario, candidate, 'resumed checkpoint: ' + label, with_file(midway, CACHE + 'pending.json', content), [REFRESH])

        engine.start_scenario()
        spent = reach('6', fresh, days=0)           # the whole of a cap of 6 spent just now
        scenario.env[CAP] = '6'
        for label, content in [('total_requests as text', {'total_requests': 'x'}), ('a list', [1]), ('an empty object', {}),
                               ('an empty list', []), ('null', b'null'), ('not JSON', NOT_JSON)]:
            compare(engine, scenario, candidate, 'checkpoint with the window spent: ' + label,
                    with_file(spent, CACHE + 'pending.json', content), [REFRESH])
        scenario.env.pop(CAP)

        engine.start_scenario()
        cached = run(engine, scenario, ORACLE, LOOKUP, fresh)['files']   # a ninja cache file, no ForeverDB state
        ninja = 'vendors/wowforever.ninja/'
        spoiled = {**cached, ninja + '100.json': NOT_JSON}
        scenario.fixture.scripts = {1: {'status': 503, 'body': {}}}
        compare(engine, scenario, candidate, 'ninja cache: a file that is not JSON, after a failed sync', spoiled, [REFRESH])
        compare(engine, scenario, candidate, 'ninja cache: a file that is not UTF-8, after a failed sync',
                {**cached, ninja + '100.json': NOT_UTF8}, [REFRESH])
        scenario.fixture.scripts = {}
        compare(engine, scenario, candidate, 'ninja cache: a file that is not JSON, after an incomplete sync', spoiled, [REFRESH])
    finally:
        scenario.close()
        engine.close()
    print(f'identical {counts["identical"]}, sanctioned {counts["sanctioned"]}, failures {len(failures)}')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
