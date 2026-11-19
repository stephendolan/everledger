"""Run with python3 -m unittest parity.test_parity -v (standard library only)."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import urllib.request

from parity.corpus import DBScenario, load_cli
from parity.engine import Engine, Normalizer, ORIGIN, ROOT, compare, snapshot
from parity.run import run


class Harness(unittest.TestCase):
    def setUp(self):
        self.engine = Engine([str(ROOT / 'parity/oracle-candidate')], both=False)
        self.addCleanup(self.engine.close)
        self.scenario = DBScenario(self.engine, load_cli(), 'self-proof')
        self.addCleanup(self.scenario.close)

    def test_wrong_candidate_is_caught_for_every_observable(self):
        cases = [
            ('stdout', ('events', '--limit', '1', '--json'), 'stdout'),
            ('exit', ('events', '--limit', '1', '--json'), 'exit_code'),
            ('stderr', ('events', '--after', ''), 'stderr'),
            ('request-order', ('vendors', '2604', '--json'), 'requests'),
            ('query', ('events', '--limit', '1', '--json'), 'requests'),
            ('cache', ('vendors', '2604', '--json'), 'files/vendors/wowforever.ninja/2604.json'),
            ('sleep', ('refresh', 'catalogue', '--json'), 'sleep_log'),
        ]
        for mode, args, field in cases:
            with self.subTest(observable=mode):
                self.engine.start_scenario()
                self.engine.candidate = [sys.executable, str(ROOT / 'parity/wrong_candidate.py'), mode]
                # Remove all state so the request-order mutation always reaches HTTP.
                from parity.engine import restore
                restore(self.scenario.root, {'catalogue.json': (ROOT / 'tests/fixtures/catalogue-fixture.json').read_bytes()})
                self.scenario.step(*args)
                record = self.engine.records[-1]
                self.assertEqual(record['state'], 'unexplained', record)
                self.assertIn(field, [row['field'] for row in record['differences'] if row['class'] is None])
                if mode == 'request-order':
                    a = record['oracle']['requests']; b = record['candidate']['requests']
                    self.assertEqual([r['path'] for r in a], ['/items/2604.json', '/items/2604.pins.json'])
                    self.assertEqual([r['path'] for r in b], ['/items/2604.pins.json', '/items/2604.json'])
                if mode == 'query':
                    self.assertTrue(record['candidate']['requests'][0]['path'].endswith('&unexpected=1'))

    def test_abrupt_exit_rejects_a_spend_dated_from_attempt_start(self):
        report = run([sys.executable, str(ROOT / 'parity/wrong_candidate.py'), 'cooldown'],
                     ['foreverdb/abrupt_exit_spend'], progress=False)
        self.assertFalse(report['success'])
        self.assertEqual(report['errors'], [])
        retry = next(row for row in report['steps'] if row['name'].endswith('/2/replay'))
        self.assertEqual(retry['state'], 'unexplained')
        self.assertIn('requests', [row['field'] for row in retry['differences']])
        self.assertFalse(any('/api/v1/' in row['path'] for row in retry['oracle']['requests']))
        self.assertTrue(any('/api/v1/' in row['path'] for row in retry['candidate']['requests']))

    def test_wrapper_equals_plain_oracle_without_time_output(self):
        fixture = self.scenario.fixture
        fixture.vocabulary.routes['/events'] = (200, {'entries': [], 'sources': []})
        for args in [('events', '--limit', '5', '--since', '2026-11-01', '--json'), ('events', '--type', 'invalid'), ('refresh',)]:
            with self.subTest(argv=args), tempfile.TemporaryDirectory() as temp:
                initial = snapshot(self.scenario.root)
                a = self.engine.capture([sys.executable, str(ROOT / 'parity/oracle.py')], args,
                                        self.scenario.env, fixture, self.scenario.root, initial, Path(temp))
                b = self.engine.capture([str(ROOT / 'everledger')], args, self.scenario.env,
                                        fixture, self.scenario.root, initial, Path(temp), plain=True)
                state, diffs = compare(a, b)
                self.assertEqual((state, diffs), ('identical', []))

    def test_clock_is_shared_with_python_child_and_has_exact_sleep_repr(self):
        with tempfile.TemporaryDirectory() as temp:
            controls = Path(temp)
            (controls / 'elapsed').write_text('0.0')
            (controls / 'sleeps').write_bytes(b'')
            env = self.engine.environment(self.scenario.env, self.scenario.fixture, self.scenario.root, controls)
            env.update(EVERLEDGER_PARITY_ORACLE='1', PYTHONPATH=str(ROOT / 'parity/oracle_hook') + ':' + str(ROOT))
            script = "import datetime,time,subprocess,sys; print(datetime.datetime.now(datetime.UTC).isoformat(),flush=True); time.sleep(2.1); subprocess.run([sys.executable,'-c','import datetime,time; print(datetime.datetime.now(datetime.UTC).isoformat()); print(time.monotonic()); time.sleep(0.0)'],check=True)"
            result = subprocess.run([sys.executable, '-c', script], env=env, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, b'2026-11-15T12:00:00+00:00\n2026-11-15T12:00:02.100000+00:00\n2.1\n')
            self.assertEqual((controls / 'sleeps').read_bytes(), b'2.1\n0.0\n')

    def test_loopback_refusal_happens_before_launch(self):
        for url in ('https://foreverdb.net', 'https://api.ahledger.com', 'https://wowforever.ninja', 'http://127.0.0.1.evil.invalid'):
            with self.subTest(url=url), tempfile.TemporaryDirectory() as temp:
                env = dict(self.scenario.env, EVERLEDGER_AHLEDGER_URL=url)
                with self.assertRaises(ValueError):
                    self.engine.environment(env, self.scenario.fixture, self.scenario.root, Path(temp))

    def test_fixture_exact_query_fallback_script_headers_and_redirect_refusal(self):
        f = self.scenario.fixture
        f.vocabulary.routes['/probe'] = (200, 'path')
        f.vocabulary.routes['/probe?a=1'] = (200, 'exact')
        for path, expected in [('/probe?a=1', b'exact'), ('/probe?a=2', b'path')]:
            req = urllib.request.Request(f.base + path, headers={'Authorization': 'Bearer fixture', 'X-API-Key': 'fixture-key', 'X-Surrogate-Test': 'fixture-surrogate'})
            with urllib.request.urlopen(req) as response:
                self.assertEqual(response.read(), expected)
        self.assertEqual(f.log[0]['headers']['Authorization'], 'Bearer fixture')
        self.assertEqual(f.log[0]['headers']['X-API-Key'], 'fixture-key')
        self.assertEqual(f.log[0]['headers']['X-Surrogate-Test'], 'fixture-surrogate')
        f.scripts = {1: {'status': 302, 'headers': {'Location': '/redirected'}}}
        self.scenario.sync()
        record = self.engine.records[-1]
        self.assertEqual(record['oracle']['exit_code'], 4)
        self.assertEqual([r['path'] for r in record['oracle']['requests'] if '/api/' in r['path']], ['/api/v1/vendors?limit=100&offset=0'])
        # A failed sync is reported in the result, not on stderr.
        self.assertIn(b'HTTP 302', record['oracle']['stdout'])
        self.assertEqual(record['oracle']['stderr'], b'')

    def test_allowlist_never_swallows_contract_text_or_other_observables(self):
        self.scenario.step('events', '--after', '')
        oracle = self.engine.records[-1]['oracle']
        for field, value in [('stderr', b'wrong failure'), ('stdout', b'wrong'), ('exit_code', 4), ('sleep_log', b'5.0\n')]:
            candidate = copy.deepcopy(oracle); candidate[field] = value
            state, _ = compare(oracle, candidate)
            self.assertEqual(state, 'unexplained')
        candidate = copy.deepcopy(oracle); candidate['stderr'] = b'usage differs'
        self.assertEqual(compare(oracle, candidate, 'parser-usage')[0], 'sanctioned')
        candidate['stdout'] = b'still a regression'
        self.assertEqual(compare(oracle, candidate, 'parser-usage')[0], 'unexplained')
        candidate['stderr'] = b'payload boundary: expected object\n'
        self.assertEqual(compare(oracle, candidate, 'shape-drift')[0], 'sanctioned')
        candidate['stderr'] = b''
        self.assertEqual(compare(oracle, candidate, 'shape-drift')[0], 'unexplained')
        for warning in (b'deprecated --known; use --char\n', b'catalogue is older than 14d; run: everledger refresh catalogue\n', b'cursor_invalid: resynchronizing once from 0\n'):
            a = copy.deepcopy(oracle); b = copy.deepcopy(oracle)
            a['stderr'] = warning + b"error: 'latest'\n"
            b['stderr'] = b'clear boundary error\n'
            self.assertEqual(compare(a, b, 'python-internal')[0], 'unexplained')
            b['stderr'] = warning + b'clear boundary error\n'
            self.assertEqual(compare(a, b, 'python-internal')[0], 'sanctioned')

    def test_transport_detail_is_the_only_wording_a_dead_source_may_change(self):
        self.scenario.step('events', '--after', '')
        base = self.engine.records[-1]['oracle']
        python = b'http://127.0.0.1:9/listings: source unreachable (<urlopen error [Errno 111] Connection refused>)'
        rust = b'http://127.0.0.1:9/listings: source unreachable (io: Connection refused (os error 111))'
        for field, wrap in (('stderr', lambda text: text + b'\n'),
                            ('stdout', lambda text: b'{"status": [{"source": "listings", "state": "' + text + b'"}]}\n'),
                            ('stdout', lambda text: b'listings | ' + text + b'\nsource: unknown (age unknown)\n')):
            oracle = copy.deepcopy(base); candidate = copy.deepcopy(base)
            oracle[field] = wrap(python); candidate[field] = wrap(rust)
            state, differences = compare(oracle, candidate)
            self.assertEqual((state, [row['class'] for row in differences]), ('sanctioned', ['transport-detail']))
            candidate[field] = wrap(rust.replace(b'/listings', b'/events'))
            self.assertEqual(compare(oracle, candidate)[0], 'unexplained')
            candidate[field] = wrap(rust).replace(b'source unreachable', b'unreachable')
            self.assertEqual(compare(oracle, candidate)[0], 'unexplained')
            candidate[field] = wrap(rust) + b'extra\n'
            self.assertEqual(compare(oracle, candidate)[0], 'unexplained')
        oracle = copy.deepcopy(base); candidate = copy.deepcopy(base)
        oracle['stdout'] = b'plain row\n'; candidate['stdout'] = b'other row\n'
        self.assertEqual(compare(oracle, candidate)[0], 'unexplained')

    def test_normalization_preserves_bytes_and_sync_relationships(self):
        a = Normalizer(); b = Normalizer()
        first = b'{"sync_id": "' + b'a' * 32 + b'", "x": 1}'
        second = b'{"sync_id": "' + b'b' * 32 + b'", "x": 1}'
        self.assertEqual(a.raw(first), b.raw(second))
        self.assertNotEqual(a.raw(first.replace(b'1}', b'2}')), b.raw(second))
        # A new sync must not collapse onto the previous one.
        self.assertNotEqual(a.raw(first), b.raw(first))
        a = Normalizer(); b = Normalizer()
        nested_a = b'{"raw": ' + first + b'}'
        nested_b = b'{"raw": ' + second + b'}'
        self.assertNotEqual(a.raw(nested_a), b.raw(nested_b))
        self.scenario.step('events', '--after', '')
        left = self.engine.records[-1]['oracle']
        right = copy.deepcopy(left)
        left['stdout'] = first; right['stdout'] = second
        self.assertEqual(compare(a.transcript(left), b.transcript(right))[0], 'unexplained')
        left['stdout'] = right['stdout'] = b''
        left['files']['other.json'] = first
        right['files']['other.json'] = second
        self.assertEqual(compare(a.transcript(left), b.transcript(right))[0], 'unexplained')


class FullCorpus(unittest.TestCase):
    def test_oracle_against_oracle_twice_has_no_exemptions(self):
        totals = []
        for iteration in range(2):
            with self.subTest(run=iteration + 1):
                report = run([str(ROOT / 'parity/oracle-candidate')], progress=False)
                self.assertEqual(report['errors'], [])
                self.assertTrue(report['success'])
                self.assertEqual(len(report['selected']), 96)
                self.assertTrue(all(row['state'] == 'identical' for row in report['steps']))
                totals.append(len(report['steps']))
                print(f'\noracle/oracle run {iteration + 1}: {len(report["steps"])} identical; 0 sanctioned; 0 unexplained', flush=True)
                for scenario in ('budgeted_bootstrap', 'mid_detail_interrupt', 'abrupt_exit_spend', 'window_expiry', '502_retry_at_budget_boundary'):
                    self.assertTrue(any(scenario in row['name'] for row in report['steps']))
                first = next(row for row in report['steps'] if row['name'] == 'foreverdb/budgeted_bootstrap_resumes_without_activating_partial_index/1/replay')
                result = json.loads(first['oracle']['stdout'])
                self.assertEqual(result['vendor_sync']['requests'], 36)
                self.assertFalse(result['vendor_sync']['complete'])
        self.assertEqual(totals[0], totals[1])
        self.assertGreater(totals[0], 500)


class FuzzDeterminism(unittest.TestCase):
    def test_read_worlds_are_independent_of_python_hash_seed(self):
        script = """
import json, random
from parity.fuzz import world, invocation
for seed in range(20):
    rng = random.Random(seed)
    routes, respond, catalogue, items, names = world(rng)
    argv = [invocation(rng, items, names, catalogue, {'after': [], 'page': []}) for _ in range(14)]
    print(json.dumps([routes, respond({'limit': ['500']}), catalogue, argv]))
"""
        runs = [subprocess.check_output([sys.executable, '-c', script], cwd=ROOT,
                env=dict(os.environ, PYTHONHASHSEED=seed)) for seed in ('1', '2')]
        self.assertEqual(runs[0], runs[1])


if __name__ == '__main__':
    unittest.main()
