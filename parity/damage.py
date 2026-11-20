#!/usr/bin/env python3
"""Cache files no sync writes, and responses no server should send.

Usage: python3 parity/damage.py CANDIDATE

A sync writes its checkpoint, journal, and index itself, and a lookup its
ninja cache file, so the corpus and the fuzz only ever hand an implementation
files it wrote. This puts other things in their place, in three parts.

**Chosen cases.** The wrong JSON type for a whole file, empty containers,
bytes that are not UTF-8, spends that do not add up. For each, both
implementations run the commands that read the file, from the same state, and
must exit with the same code, send the same requests, and print the same
stdout. Where the reference carries on with a payload it could not have
written and the candidate refuses it, the case is named in `SANCTIONED` with
its reason.

**Every field.** Each key of each object in the four files is in turn
dropped, set to null, and given another JSON type. The candidate must do what
the reference does, or refuse. No case is named: the rule is the class.

To refuse is to exit 1 with nothing on stdout, having sent no request the
reference did not send and left the damaged file as it was. A candidate that
carries on where the reference fails, or sends a request the reference does
not, fails the check.

**Responses that break HTTP.** A status line that is not HTTP, a body or an
error body shorter than its declared length, a broken chunk, and a valid
response cut off after each byte of its head, from each source. Both sides
must exit the same, send the same requests, and print the same stdout.

One difference is accepted. Where a response ends inside its headers, the
reference's HTTP library reads a complete response with no body, and the
candidate's reads a broken connection. The case passes only if the reference
does exactly what it does for an empty response and the candidate does exactly
what both do when the connection closes without a word.

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
NINJA = 'vendors/wowforever.ninja/'
KEY = 'EVERLEDGER_FOREVERDB_API_KEY'
REFRESH = ['refresh', 'catalogue', '--json']
STATUS = ['status', '--json']
LOOKUP = ['vendors', '2604', '--json']
HUMAN = ['vendors', '2604']
NOT_JSON = b'{not json'
NOT_UTF8 = b'\xff'
# The reference computes with whatever a spend holds, and drops an expired spend without reading it.
# The port needs a list of spends, each with a whole, non-negative count.
SPENDS = "the journal's spends are not a list of whole, non-negative request counts"
FAILED = "the journal's failed flag is not a boolean"
SANCTIONED = {
    **{(prefix + label, 'refresh'): SPENDS
       for prefix in ('journal: ', 'journal inside the window: ')
       for label in ('spends is an object', 'a spend of 1.5 requests', 'a spend of -1 requests',
                     'a spend of true requests')},
    ('journal: a spend without requests', 'refresh'): SPENDS,
    ('journal: a spend of "1" requests', 'refresh'): SPENDS,
    ('journal: failed is 1', 'refresh'): FAILED,
    ('journal: failed is 1', 'status'): FAILED,
    ('journal: failed is 1', 'vendors'): FAILED,
    ('journal: status is text', 'refresh'): "the journal's status is not a number",
    # The reference sends a request with the text as its offset before the service's answer fails the sync.
    ('resumed checkpoint: next_offset as text', 'refresh'): "the checkpoint's next_offset is not a number",
    # The reference prints the text as the total.
    ('checkpoint with the window spent: total_requests as text', 'refresh'): "the checkpoint's total_requests is not a number",
    # The reference spends its requests and fails when it builds the next index from the old one.
    ('index: items is a list', 'refresh'): "the index's items are not an object",
}
JSON = b'Content-Type: application/json\r\n'
MALFORMED = {
    'a status line that is not HTTP': b'HTTP/1.1 abc\r\n\r\n',
    'a body shorter than its Content-Length': b'HTTP/1.1 200 OK\r\n' + JSON + b'Content-Length: 500\r\n\r\n{"data": [',
    'an error body shorter than its Content-Length': b'HTTP/1.1 404 Not Found\r\n' + JSON + b'Content-Length: 500\r\n\r\n{"error":',
    'a retryable error body cut short':
        b'HTTP/1.1 502 Bad Gateway\r\n' + JSON + b'Content-Length: 500\r\n\r\n{"retryable": true, "retry_after": 60',
    'a chunk cut short': b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab',
    'a chunk size that is not a number': b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nab\r\n0\r\n\r\n',
    'headers cut short': b'HTTP/1.1 200 OK\r\nContent-Len',
}
CLOSED = b''
EMPTY = b'HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n'
LENIENT = ("the reference's HTTP library reads a response that ends inside its headers as complete and empty; "
           'the port reads a closed connection')
MISSING = object()
counts = {'identical': 0, 'sanctioned': 0, 'refused': 0}
failures = []


def raw(value):
    return value if isinstance(value, bytes) else json.dumps(value).encode()


def run(engine, scenario, command, argv, state):
    with tempfile.TemporaryDirectory() as controls:
        return engine.capture(command, argv, scenario.env, scenario.fixture, scenario.root, state, Path(controls))


def compare(engine, scenario, candidate, label, state, commands, named=True, damaged=None):
    """Both sides on one damaged state. A refusal passes if `SANCTIONED` names it, or if no name is asked for.

    `damaged` names the damaged file; a candidate that refuses must leave it as it was.
    """
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
        refused = (port['exit_code'] == 1 and not port['stdout'] and paths[1] == paths[0][:len(paths[1])]
                   and (damaged is None or port['files'].get(damaged) == state.get(damaged)))
        summary = (f'{label}: {" ".join(argv)}: exit {reference["exit_code"]} and {port["exit_code"]}, '
                   f'ForeverDB requests {sent[0]} and {sent[1]}, other requests {len(paths[0]) - sent[0]} and {len(paths[1]) - sent[1]}')
        if named and reason and refused and sent[1] == 0:
            counts['sanctioned'] += 1
            print(f'sanctioned  {summary} ({reason})')
        elif not named and refused:
            counts['refused'] += 1
            print(f'refused     {summary}')
        else:
            failures.append(summary)
            print(f'FAIL        {summary}')


def another(value):
    """A value of another JSON type than this one, and how to say so."""
    if isinstance(value, str):
        return 5, 'as a number'
    if isinstance(value, list):
        return {}, 'as an object'
    if isinstance(value, dict):
        return [], 'as a list'
    return '7', 'as text'   # digits, so that an offset the reference passes on is one the fixture can answer


def damaged(document, path):
    """The object at `path` with each key in turn dropped, set to null, and given another JSON type."""
    target = document
    for step in path:
        target = target[step]
    for key, value in target.items():
        other, how = another(value)
        for name, change in [('dropped', MISSING), *([('null', None)] if value is not None else []), (how, other)]:
            changed = copy.deepcopy(document)
            inner = changed
            for step in path:
                inner = inner[step]
            if change is MISSING:
                del inner[key]
            else:
                inner[key] = change
            yield f'{key} {name}', changed


def cuts(payload):
    """A valid response to `payload`, cut off after each byte of its head and at three points in its body."""
    body = json.dumps(payload).encode()
    head = b'HTTP/1.1 200 OK\r\n' + JSON + b'Content-Length: %d\r\n\r\n' % len(body)
    whole = head + body
    for size in range(1, len(head)):
        yield f'a response cut after {size} of the {len(head)} bytes of its head', whole[:size]
    for name, size in (('before its body', len(head)), ('halfway through its body', len(head) + len(body) // 2),
                       ('one byte short', len(whole) - 1)):
        yield f'a response cut {name}', whole[:size]


def malformed_responses(engine, scenario, module, candidate, fresh, cached):
    """Responses that break HTTP, from each source. One accepted difference: see LENIENT."""
    routes = module.Handler.routes
    key = scenario.env[KEY]

    def place(name, argv, state, route=None, script=None, cap=None, configured=True, grid=False):
        return name, argv, state, route, script or {}, cap, configured, grid

    failed = {1: {'status': 503, 'body': {}}}
    item, pins = '/items/2604.json', '/items/2604.pins.json'
    places = (
        place('ForeverDB, first request of a sync by key', REFRESH, fresh, grid=True),
        place('ForeverDB, first request of a sync by connector', REFRESH, fresh, configured='connector'),
        place('the AHledger price quote', ['price', '2604', '--json'], fresh, '/v1/prices/forever.pvp.alliance.us/2604'),
        place('the roster', ['char', '--json'], fresh, '/characters'),
        place('a catalogue page', REFRESH, fresh, '/professions/alchemy/'),
        place('the ninja item, nothing cached', LOOKUP, fresh, item, configured=False, grid=True),
        place('the ninja pins, nothing cached', LOOKUP, fresh, pins, configured=False),
        place('the ninja item, refreshed with ForeverDB not configured', REFRESH, cached, item, configured=False),
        place('the ninja pins, refreshed with ForeverDB not configured', REFRESH, cached, pins, configured=False),
        place('the ninja item, refreshed after an incomplete sync', REFRESH, cached, item, cap='2'),
        place('the ninja item, refreshed after a failed sync', REFRESH, cached, item, script=failed),
    )
    seen = lambda side: (side['exit_code'], [row['path'] for row in side['requests']], side['stdout'], side['sleep_log'])
    for name, argv, state, route, script, cap, configured, grid in places:
        def both(data):
            saved = dict(routes)
            if route:
                routes[route] = (200, data)
                scenario.fixture.scripts = dict(script)
            else:
                scenario.fixture.scripts = {1: {'body': data}}
            if cap:
                scenario.env[CAP] = cap
            if configured is False:
                del scenario.env[KEY]
            elif configured == 'connector':
                scenario.connector()
            try:
                return [seen(run(engine, scenario, side, argv, state)) for side in (ORACLE, candidate)]
            finally:
                routes.clear()
                routes.update(saved)
                scenario.fixture.scripts = {}
                scenario.env.pop(CAP, None)
                scenario.env[KEY] = key
                scenario.env['EVERLEDGER_FOREVERDB_VIA_CONNECTOR'] = ''

        closed, empty = both(CLOSED), both(EMPTY)
        for label, pair in (('a connection closed without a response', closed), ('an empty response', empty)):
            if pair[0] == pair[1]:
                counts['identical'] += 1
            else:
                failures.append(f'{label}, from {name}')
                print(f'FAIL        {label}, from {name}: exit {pair[0][0]} and {pair[1][0]}')
        def judge(cases):
            """The labels of the cases that pass by the accepted difference alone."""
            lenient = []
            for label, data in cases:
                reference, port = both(data)
                if reference == port:
                    counts['identical'] += 1
                elif reference == empty[0] and port == closed[0]:
                    counts['sanctioned'] += 1
                    lenient.append(label)
                else:
                    failures.append(f'{label}, from {name}')
                    print(f'FAIL        {label}, from {name}: exit {reference[0]} and {port[0]}, '
                          f'requests {len(reference[1])} and {len(port[1])}')
            return lenient

        for label in judge(MALFORMED.items()):
            print(f'sanctioned  {label}, from {name} ({LENIENT})')
        if grid:
            lenient = judge(cuts(routes[route][1] if route else scenario.fixture.forever['pages'][0]))
            if lenient:
                print(f'sanctioned  {len(lenient)} cuts, from "{lenient[0]}" to "{lenient[-1]}", from {name} ({LENIENT})')


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
    scenario.env['EVERLEDGER_FETCH_TRANSPORT'] = 'native'
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
        looked_up = run(engine, scenario, ORACLE, LOOKUP, fresh)     # a ninja cache file, no ForeverDB state
        engine.advance(seconds=elapsed(looked_up))
        cached = looked_up['files']
        key = scenario.env[KEY]
        failed_sync = {1: {'status': 503, 'body': {}}}

        def refreshed(label, state, named=True, damaged=None):
            """A refresh that reaches the ninja cache: after a failed sync, an incomplete one, and none."""
            scenario.fixture.scripts = dict(failed_sync)
            compare(engine, scenario, candidate, label + ', after a failed sync', state, [REFRESH], named, damaged)
            scenario.fixture.scripts = {}
            scenario.env[CAP] = '2'
            compare(engine, scenario, candidate, label + ', after an incomplete sync', state, [REFRESH], named, damaged)
            scenario.env.pop(CAP)
            del scenario.env[KEY]
            compare(engine, scenario, candidate, label + ', ForeverDB not configured', state, [REFRESH], named, damaged)
            scenario.env[KEY] = key

        for label, content in [('a file that is not JSON', NOT_JSON), ('a file that is not UTF-8', NOT_UTF8),
                               ('null', b'null'), ('an empty object', {}), ('a list', [1]),
                               ('another backend_url and nothing else', {'backend_url': 'http://elsewhere.invalid'})]:
            for name in ('100.json', '2604.json'):
                state = {**cached, NINJA + name: raw(content)}
                refreshed(f'ninja cache: {name} is {label}', state)
                compare(engine, scenario, candidate, f'ninja cache: {name} is {label}', state, [LOOKUP, STATUS])

        # Every field of every file: dropped, null, and another type.
        engine.start_scenario()
        synced = reach(None, fresh)
        index = json.loads(synced[CACHE + 'index.json'])
        row = ('items', '2604', 'vendors', 0)
        for part, where, commands in (('', (), [REFRESH, STATUS, LOOKUP, HUMAN]),
                                      ("an entry's ", row[:2], [REFRESH, STATUS, LOOKUP, HUMAN]),
                                      ("an item's ", (*row[:2], 'item'), [REFRESH, STATUS, LOOKUP, HUMAN]),
                                      ("a row's ", row, [STATUS, LOOKUP, HUMAN])):
            for label, content in damaged(index, where):
                compare(engine, scenario, candidate, f'index: {part}{label}', with_file(synced, CACHE + 'index.json', content),
                        commands, named=False, damaged=CACHE + 'index.json')
        journal = json.loads(synced[CACHE + 'attempt.json'])
        for part, where in (('', ()), ("a spend's ", ('spends', 0))):
            for label, content in damaged(journal, where):
                compare(engine, scenario, candidate, f'journal: {part}{label}', with_file(synced, CACHE + 'attempt.json', content),
                        [REFRESH, STATUS, LOOKUP], named=False, damaged=CACHE + 'attempt.json')

        engine.start_scenario()
        recent = reach(None, fresh, days=0)
        live = json.loads(recent[CACHE + 'attempt.json'])
        for label, content in damaged(live, ('spends', 0)):
            compare(engine, scenario, candidate, f"journal inside the window: a spend's {label}",
                    with_file(recent, CACHE + 'attempt.json', content), [REFRESH], named=False, damaged=CACHE + 'attempt.json')

        engine.start_scenario()
        midway = reach('2', fresh)
        checkpoint = json.loads(midway[CACHE + 'pending.json'])
        sale = ('details', next(iter(checkpoint['details'])), 'data', 'sells', 0)
        for part, where in (('', ()), ("the metadata's ", ('metadata',)), ("a detail's ", sale[:2]), ("a detail's data's ", sale[:3]),
                            ("a sale's ", sale), ("a sale's item's ", (*sale, 'item')), ("a sale's row's ", (*sale, 'row'))):
            for label, content in damaged(checkpoint, where):
                compare(engine, scenario, candidate, f'checkpoint: {part}{label}', with_file(midway, CACHE + 'pending.json', content),
                        [REFRESH], named=False, damaged=CACHE + 'pending.json')

        engine.start_scenario()
        engine.advance(seconds=elapsed(looked_up))
        ninja = json.loads(cached[NINJA + '2604.json'])
        for part, where, whole in (('', (), True), ("an item's ", ('item',), True), ("a row's ", ('vendors', 0), False)):
            for label, content in damaged(ninja, where):
                state = with_file(cached, NINJA + '2604.json', content)
                del scenario.env[KEY]
                compare(engine, scenario, candidate, f'ninja cache: {part}{label}', state, [LOOKUP, HUMAN, STATUS], named=False,
                        damaged=NINJA + '2604.json')
                scenario.env[KEY] = key
                if whole:
                    refreshed(f'ninja cache: {part}{label}', state, named=False, damaged=NINJA + '2604.json')

        engine.start_scenario()
        malformed_responses(engine, scenario, module, candidate, fresh, cached)
    finally:
        scenario.close()
        engine.close()
    print(f'identical {counts["identical"]}, sanctioned {counts["sanctioned"]}, refused {counts["refused"]}, failures {len(failures)}')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
