#!/usr/bin/env python3
"""Deterministic public-fetch transport contracts for Python and Rust.

Usage: python3 parity/fetch_transport.py CANDIDATE
"""
import base64
import json
import os
import shutil
from pathlib import Path
import sys
import tempfile

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.corpus import DBScenario, load_cli
from parity.engine import Engine, ROOT, compare, snapshot
from parity.tls import address, mint, speak_tls

STUB = r'''
import base64, json, os, sys
from urllib.parse import urlsplit
config = json.load(open(os.environ['EVERLEDGER_TEST_CURL_CONFIG']))
with open(os.environ['EVERLEDGER_TEST_CURL_LOG'], 'a') as log:
    log.write(json.dumps(sys.argv[1:]) + '\n')
if config.get('fail'):
    sys.stdout.buffer.write(b'partial curl body')
    sys.exit(28)
path = urlsplit(sys.argv[-1]).path or '/'
row = config['routes'][path]
status = row['status']
sys.stdout.buffer.write(base64.b64decode(row['body']))
sys.stderr.buffer.write(('HTTP/1.1 %s Fixture\r\n' % status).encode())
for name, value in row['headers'].items():
    sys.stderr.buffer.write(('%s: %s\r\n' % (name.lower(), value)).encode())
sys.stderr.buffer.write(('\r\n\n%03d' % status).encode())
'''
PARTIAL = b'HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\ncut short'
FIELDS = ('exit_code', 'stdout', 'stderr', 'files', 'modes', 'sleep_log')


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    candidate = [str(Path(sys.argv[1]).resolve())]
    engine = Engine(candidate, both=False)
    module = load_cli()
    scenario = DBScenario(engine, module, 'fetch-transport')
    scenario.env['EVERLEDGER_FOREVERDB_API_KEY'] = ''
    checks = 0

    def check(condition, label):
        nonlocal checks
        checks += 1
        if not condition:
            raise AssertionError(label)
        print('ok    ' + label, flush=True)

    try:
        with tempfile.TemporaryDirectory(prefix='everledger-fetch-') as temp:
            controls = Path(temp)
            authority, certificate, key = mint(temp)
            speak_tls(scenario.fixture, certificate, key)
            address(scenario, 'localhost')
            scenario.env['SSL_CERT_FILE'] = str(authority)
            scenario.env['EVERLEDGER_TEST_CURL_CONFIG'] = str(controls / 'curl.json')
            scenario.env['EVERLEDGER_TEST_CURL_LOG'] = str(controls / 'calls')
            bin_dir = controls / 'bin'
            bin_dir.mkdir()
            missing_dir = controls / 'missing'
            missing_dir.mkdir()
            for tool in ('python3', 'bash', 'dirname', 'mkdir', 'grep', 'mv'):
                (missing_dir / tool).symlink_to(shutil.which(tool))
            stub = bin_dir / 'curl'
            stub.write_text('#!' + sys.executable + '\n' + STUB)
            stub.chmod(0o755)
            scenario.env['PATH'] = str(bin_dir) + os.pathsep + os.environ['PATH']
            fresh = snapshot(scenario.root)
            routes = module.Handler.routes
            for path, row in list(routes.items()):
                if path.startswith('/professions/'):
                    routes[path] = (row[0], row[1] + '\n<!-- \x00HTTP/1.1 404\r\n\r\n\n200\n -->')
            saved = dict(routes)
            canned = {}
            for path in saved:
                status, payload, headers, _ = scenario.fixture.response(path)
                body = payload if isinstance(payload, str) else json.dumps(payload)
                canned[path.split('?')[0]] = dict(status=status, body=base64.b64encode(body.encode()).decode(),
                    headers={'Last-Modified': 'Sat, 14 Nov 2026 20:20:39 GMT', **headers})
            config = {'routes': canned}

            def capture(command, argv, mode='auto', fail=False, missing=False):
                scenario.env['EVERLEDGER_FETCH_TRANSPORT'] = mode
                scenario.env['PATH'] = str(controls / 'missing') if missing else str(bin_dir) + os.pathsep + os.environ['PATH']
                config['fail'] = fail
                (controls / 'curl.json').write_text(json.dumps(config))
                (controls / 'calls').write_text('')
                result = engine.capture(command, argv, scenario.env, scenario.fixture, scenario.root, fresh, controls)
                calls = [json.loads(line) for line in (controls / 'calls').read_text().splitlines()]
                return result, calls

            commands = [(['price', '2604', '--json'], '/v1/prices/forever.pvp.alliance.us/2604'),
                        (['margins', '--json'], '/v1/pricetable/forever.pvp.alliance.us'),
                        (['listings', '--json'], '/v1/pricetable/forever.pvp.alliance.us'),
                        (['positions', '--json'], '/v1/pricetable/forever.pvp.alliance.us'),
                        (['vendors', '2604', '--json'], '/items/2604.pins.json'),
                        (['vendors', '2604', '--json'], '/items/2604.json'),
                        (['status', '--json'], '/v1/pricetable/forever.pvp.alliance.us'),
                        (['refresh', 'catalogue', '--json'], '/professions/alchemy/')]
            for argv, path in commands:
                pair = []
                for name, command in [('Python', [sys.executable, str(ROOT / 'parity/oracle.py')]), ('Rust', candidate)]:
                    routes.clear(); routes.update(saved)
                    baseline, calls = capture(command, argv)
                    check(baseline['exit_code'] in (0, 1, 3) and not calls, f'{name} {argv[0]} native success skips curl')
                    forced, calls = capture(command, argv, 'curl')
                    check(all(forced[f] == baseline[f] for f in FIELDS) and calls
                          and not any(row['path'] == path for row in forced['requests']),
                          f'{name} {argv[0]} forced curl preserves answer and files')
                    forced_failure, calls = capture(command, argv, 'curl', fail=True)
                    check(forced_failure['exit_code'] == 4 and calls
                          and not any(row['path'] == path for row in forced_failure['requests']),
                          f'{name} {argv[0]} forced curl failure skips native')
                    routes[path] = (200, PARTIAL)
                    native, calls = capture(command, argv, 'native')
                    check(native['exit_code'] == 4 and not calls, f'{name} {argv[0]} native disconnect stays unreachable')
                    fallback, calls = capture(command, argv)
                    check(all(fallback[f] == baseline[f] for f in FIELDS) and len(calls) == 1,
                          f'{name} {argv[0]} auto fallback preserves answer and files')
                    invocation = calls[0]
                    check('--fail' not in invocation and '-f' not in invocation
                          and invocation[0] == '--disable' and '--max-time' in invocation
                          and '--connect-timeout' in invocation and '--header' in invocation,
                          f'{name} {argv[0]} curl is bounded and keeps HTTP errors')
                    expected_ua = ('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36' if argv[0] == 'refresh'
                                   else 'Mozilla/5.0 (compatible; everledger)' if argv[0] == 'vendors' else 'everledger')
                    check(('user-agent: ' + expected_ua).lower() in [arg.lower() for arg in invocation], f'{name} {argv[0]} curl uses native User-Agent')
                    failed, calls = capture(command, argv, fail=True)
                    check(all(failed[f] == native[f] for f in FIELDS) and len(calls) == 1,
                          f'{name} {argv[0]} failed curl preserves original error')
                    missing, calls = capture(command, argv, missing=True)
                    check(all(missing[f] == native[f] for f in FIELDS) and not calls,
                          f'{name} {argv[0]} absent curl preserves original error')
                    pair.append(fallback)
                state, _ = compare(*pair)
                check(state == 'identical', f'{argv[0]} fallback Python/Rust parity')

            routes.clear(); routes.update(saved)
            path = commands[0][1]
            for name, command in [('Python', [sys.executable, str(ROOT / 'parity/oracle.py')]), ('Rust', candidate)]:
                scenario.env['EVERLEDGER_AHLEDGER_URL'] = 'http://127.0.0.1:' + str(engine.dead_proxy.getsockname()[1])
                connected, calls = capture(command, commands[0][0])
                check(connected['exit_code'] == 0 and len(calls) == 1,
                      f'{name} connection refusal falls back once')
                scenario.env['EVERLEDGER_AHLEDGER_URL'] = 'https://localhost:' + str(scenario.fixture.server.server_port)
                roster_path = '/characters'
                roster_saved = routes[roster_path]
                routes[roster_path] = (200, PARTIAL)
                roster_error, calls = capture(command, ['char', '--json'], 'curl')
                check(roster_error['exit_code'] == 4 and not calls, f'{name} roster never uses curl')
                routes[roster_path] = roster_saved
                scenario.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
                db, calls = capture(command, ['refresh', 'catalogue', '--json'], 'curl')
                check(db['exit_code'] == 0 and any(row['path'].startswith('/api/v1/') for row in db['requests'])
                      and not any('/api/v1/' in call[-1] for call in calls), f'{name} ForeverDB stays native')
                scenario.env['EVERLEDGER_FOREVERDB_API_KEY'] = ''
                scenario.env['EVERLEDGER_AHLEDGER_URL'] += '/bad\n'
                bad_url, calls = capture(command, commands[0][0])
                check(bad_url['exit_code'] != 0 and not calls, f'{name} bad URL never invokes curl')
                scenario.env['EVERLEDGER_AHLEDGER_URL'] = 'https://localhost:' + str(scenario.fixture.server.server_port)
                bad_args, calls = capture(command, ['price'], 'curl')
                check(bad_args['exit_code'] == 2 and not calls, f'{name} bad arguments never invoke curl')
            for name, command, count in [('Python', [sys.executable, str(ROOT / 'parity/oracle.py')], 5), ('Rust', candidate, 4)]:
                routes[path] = (302, ('HTTP/1.1 302 Found\r\nLocation: ' + path + '\r\nContent-Length: 32\r\n\r\n'
                                     + '{"error": "redirect final body"}').encode())
                canned[path] = dict(status=302, body=base64.b64encode(b'{"error": "redirect final body"}').decode(),
                                    headers={'Location': path})
                native, calls = capture(command, commands[0][0], 'native')
                forced, calls = capture(command, commands[0][0], 'curl')
                check(native['exit_code'] == 4 and b'redirect final body' in native['stderr']
                      and all(forced[f] == native[f] for f in FIELDS) and len(calls) == count,
                      f'{name} redirect exhaustion retains final body and native limit')
            for status in (302, 400, 404, 409, 422, 429, 503):
                routes[path] = (status, {'error': 'canned'})
                canned[path] = dict(status=status, body=base64.b64encode(b'{"error": "canned"}').decode(), headers={})
                for name, command in [('Python', [sys.executable, str(ROOT / 'parity/oracle.py')]), ('Rust', candidate)]:
                    native, calls = capture(command, commands[0][0])
                    check(not calls and native['exit_code'] == (2 if status in (400, 422) else 1 if status in (404, 409) else 4),
                          f'{name} completed HTTP {status} never falls back')
                    routes[path] = (200, PARTIAL)
                    fallback, calls = capture(command, commands[0][0])
                    check(all(fallback[f] == native[f] for f in FIELDS) and len(calls) == 1,
                          f'{name} curl HTTP {status} uses native status/body handling')
                    routes[path] = (status, {'error': 'canned'})
            page = '/professions/alchemy/'
            for status in (400, 404, 409, 422, 429, 503):
                routes[page] = (status, {'error': 'page unavailable'})
                canned[page] = dict(status=status, body=base64.b64encode(b'{"error": "page unavailable"}').decode(), headers={})
                for name, command in [('Python', [sys.executable, str(ROOT / 'parity/oracle.py')]), ('Rust', candidate)]:
                    native, calls = capture(command, ['refresh', 'catalogue', '--json'])
                    check(native['exit_code'] == 4 and not calls, f'{name} catalogue HTTP {status} keeps exit4 and skips fallback')
                    routes[page] = (200, PARTIAL)
                    fallback, calls = capture(command, ['refresh', 'catalogue', '--json'])
                    check(all(fallback[f] == native[f] for f in FIELDS) and len(calls) == 1,
                          f'{name} catalogue curl HTTP {status} keeps native behavior')
                    routes[page] = (status, {'error': 'page unavailable'})
            print(f'{checks} checks passed')
    finally:
        scenario.close()
        engine.close()


if __name__ == '__main__':
    main()
