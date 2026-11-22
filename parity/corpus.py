"""Replay the oracle tests; explicit CLI translations of in-process contracts."""
import copy
import datetime
import importlib.util
import json
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import types
import unittest

from parity.engine import ORIGIN, ROOT
from parity.fixture import Fixture


ORACLE_COMMIT = '5d48d3324f88c8b1d3ba1795e14a8d16ab67a338'
CAP = 'EVERLEDGER_FOREVERDB_DAILY_REQUESTS'


def load_cli():
    spec = importlib.util.spec_from_file_location('parity_reference_cli', ROOT / 'python/tests/test_cli.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.NOW = ORIGIN
    real = datetime.datetime

    class Fixed(real):
        @classmethod
        def now(cls, tz=None):
            wall = real.fromisoformat(ORIGIN.replace('Z', '+00:00'))
            return wall.astimezone(tz) if tz else wall.replace(tzinfo=None)

    module.datetime = types.SimpleNamespace(datetime=Fixed, UTC=datetime.UTC, timedelta=datetime.timedelta)
    return module


INTERNAL = [
    'sync_and_sale_labels_and_locations', 'no_index_record_and_known_empty_after_removal',
    'daily_refresh_refetches_details_without_revision_guarantee', 'interrupted_sync_preserves_previous_index',
    'budgeted_bootstrap_resumes_without_activating_partial_index', 'mid_detail_interrupt_resumes_and_old_index_survives',
    'unknown_labels_and_malformed_shapes_do_not_invent_provenance', 'abrupt_exit_spend_uses_the_last_request_start',
    'changed_extraction_discards_checkpoint_and_recovers_next_day', 'failed_index_stays_ineligible_through_incomplete_retry',
    'invalid_header_transport_error_cannot_print_key', 'key_files_precedence_and_no_key_dormancy',
    'pacing_and_failures_do_not_expose_credentials', 'observed_list_page_uses_total_and_offset',
    'stack_prices_and_limited_stock_are_exact', '502_retries_once_after_requested_delay_and_counts_request',
    '502_retry_at_budget_boundary_resumes_next_day', '502_retry_failure_stops_after_two_requests',
    'connector_sync_uses_stub_module_and_browser_ua_without_key', 'env_transport_outgoing_user_agent',
    'missing_connector_module_is_clear_and_status_still_works', 'both_transports_refuse_redirects',
    'connector_exceptions_are_redacted_and_journaled',
    'unset_daily_requests_defaults_to_36', 'valid_daily_requests_is_parsed_once',
    'invalid_daily_requests_is_a_usage_error', 'sync_honors_raised_daily_cap_from_environment',
    'window_spend_across_two_attempts_defers_at_cap', 'raised_cap_resumes_inside_window_with_remaining_budget',
    'window_expiry_restores_only_expired_spend_then_full_budget',
    'abrupt_exit_preserves_prior_window_spend', '502_retry_counts_against_remaining_window_budget',
    'index_omits_checkpoint_payload_and_ignores_legacy_fields',
    'sync_result_shape_on_success_failure_deferred_and_unconfigured',
    'refresh_cached_returns_setup_failure_without_raising', 'sync_checkpoint_write_failure_preserves_request_count',
    '502_long_wait_is_pending_and_resumes_after_not_before', '502_numeric_delays_never_overflow_or_sleep_over_300',
]
TRANSLATIONS = {
    'cache_failure_window': 'The two in-process lookup(refresh=True) calls become refresh catalogue, followed by vendors; the same two-day and eight-day cache states and 503 response are retained.',
    'sync_foreverdb': 'The in-process transport patch becomes refresh catalogue against real fixture HTTP with the virtual clock.',
    'refresh_sync_failure_reports_catalogue_and_spend_with_or_without_cache': 'The original runs the CLI in a subprocess with its transport patched to fail. Here refresh catalogue runs against a scripted 503 on the first ForeverDB request, with the original assertions.',
    'malformed_http_sync_finalizes_attempt_and_spend': 'The original runs the CLI in a subprocess with the ForeverDB address and pacing patched. Here refresh catalogue runs against the fixture, which sends each malformed response as the only request of an attempt or, at a cap of 2, as its second.',
    'abrupt_exit_spend_uses_the_last_request_start': 'SIGKILL at the third held vendor request, at a cap of 3. The next invocation starts one second short of 24 hours after that request began, when attempted_at is already more than 24 hours old, and is deferred; one second later the spend has expired. (The original moves its clock five minutes per request and checks 23h55m.)',
    'changed_extraction_discards_checkpoint_and_recovers_next_day': 'A cap of 1 reads one list page; the next day the second page carries another build, twice. Then, on a cleared cache, 36 list pages spend the default cap before the changed build appears on page 37, and the restarted traversal meets it again a day later.',
    '502_retry_at_budget_boundary_resumes_next_day': 'A cap of 1 whose only request is a retryable 502. Then, on a cleared cache, the same boundary at the default cap: the list request and 35 details, with the 502 as request 36.',
    'daily_refresh_refetches_details_without_revision_guarantee': 'The cap of 6 is set through the variable.',
    'pacing_and_failures_do_not_expose_credentials': 'The scripted 503 is removed before the last refresh, which the original runs on its restored transport.',
    'window_spend_across_two_attempts_defers_at_cap': 'A cap of 12 through the variable; the scenario clock moves 34 minutes between the two attempts.',
    'raised_cap_resumes_inside_window_with_remaining_budget': 'Caps of 2 and then 6 through the variable, 34 minutes apart on the scenario clock.',
    'window_expiry_restores_only_expired_spend_then_full_budget': 'The original holds its clock still during an attempt; here an attempt takes virtual time, so each boundary is taken from the journal: one second before the first spend is 24 hours old, that second, and the second on which the next spend expires.',
    'index_omits_checkpoint_payload_and_ignores_legacy_fields': 'The legacy fields are written into the published index between two lookups; the next sync republishes without them.',
    'sync_result_shape_on_success_failure_deferred_and_unconfigured': 'The four results are read as vendor_sync from refresh catalogue: no key, a scripted 503, a cap of 1, and the default cap.',
    'refresh_cached_returns_setup_failure_without_raising': 'The key file variable names a directory; refresh, status, and a lookup follow.',
    'sync_checkpoint_write_failure_preserves_request_count': 'The original patches the writer to fail. Here a directory takes the file\'s place while the first request is held, so the rename that publishes the next write fails: once for the checkpoint, once for the journal\'s reservation of the second request.',
    '502_long_wait_is_pending_and_resumes_after_not_before': 'A scripted retry_after of 301; the scenario clock moves 302 seconds before the last attempt.',
    '502_numeric_delays_never_overflow_or_sleep_over_300': 'Each delay is sent as a JSON literal in a scripted 502, among them Infinity, -Infinity, NaN, and a 1,001-digit integer, each on a cleared cache.',
    'abrupt_exit_preserves_prior_window_spend': 'SIGKILL at the held first request of the second attempt stands in for SystemExit.',
    '502_retry_counts_against_remaining_window_budget': 'A retryable 502 is scripted at the first request of the second attempt, which is the only one its budget allows.',
    'key_files_precedence_and_no_key_dormancy': 'Dormant refresh and status plus separate default-file, explicit-file, and environment-key refreshes make key precedence visible in HTTP headers.',
    'observed_list_page_uses_total_and_offset': 'The observed 100-row list fixture plus synthetic offsets through 1000 are consumed through refresh at the real daily cap.',
    'unset_daily_requests_defaults_to_36': 'With the variable unset, a 40-vendor bootstrap stops at 36 requests.',
    'valid_daily_requests_is_parsed_once': 'A cap of 1 allows the list request alone; the next day a cap of 5000 fetches the 40 details in one attempt. That a loaded module keeps its cap when the environment changes afterwards has no CLI form: an invocation is one process.',
    'invalid_daily_requests_is_a_usage_error': 'Each invalid value fails refresh, status, and a vendor lookup alike, before any request.',
    'sync_honors_raised_daily_cap_from_environment': 'A cap of 40 leaves the 40-vendor bootstrap one request short and a cap of 5000 completes it in one attempt of 41. The original gives each cap its own cache directory; here the cache is cleared between the two.',
}


def accounting(module):
    names = sorted(name[5:] for name in dir(module.CLI) if name.startswith('test_'))
    rows = [{'test': 'CLI.test_' + name, 'scenarios': ['cli/' + name], 'not_cli_observable': None} for name in names]
    # Detect both additions and removals, rather than silently printing the old count.
    import ast
    tree = ast.parse((ROOT / 'python/tests/test_foreverdb.py').read_text())
    owners = {node.name[5:]: owner.name for owner in tree.body if isinstance(owner, ast.ClassDef)
              for node in owner.body if isinstance(node, ast.FunctionDef) and node.name.startswith('test_')}
    if set(owners) != set(INTERNAL) or len(rows) + len(INTERNAL) != 97:
        raise ValueError('test inventory drift: update the corpus accounting')
    rows += [{'test': owners[name] + '.test_' + name, 'scenarios': ['foreverdb/' + name], 'not_cli_observable': None} for name in INTERNAL]
    return rows


def replay_cli(engine, module, name):
    owner = module.Handler
    fixture = Fixture(owner)
    sequence = [0]

    class Replay(module.CLI):
        def run_cli(self, *args, code=0):
            sequence[0] += 1
            result = engine.step('cli/' + name + '/' + str(sequence[0]), args, self.env,
                                 fixture, self.tmp.name)
            self.assertEqual(result.returncode, code, result.stderr + result.stdout)
            return result

        def sync_foreverdb(self):
            self.refresh_routes()
            self.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
            return json.loads(self.run_cli('refresh', 'catalogue', '--json').stdout)

        def test_cache_failure_window(self):
            self.run_cli('vendors', '2604', '--json')
            path = Path(self.tmp.name, 'vendors/wowforever.ninja/2604.json')
            cached = json.loads(path.read_text())
            self.assertIn('raw_pins', cached)
            cached['fetched_at'] = '2026-11-13T12:00:00Z'
            path.write_text(json.dumps(cached))
            self.run_cli('vendors', '2604', '--json', code=3)
            self.refresh_routes()
            owner.routes['/items/2604.json'] = (503, {})
            self.run_cli('refresh', 'catalogue', '--json', code=3)
            cached['fetched_at'] = '2026-11-07T12:00:00Z'
            path.write_text(json.dumps(cached))
            self.run_cli('refresh', 'catalogue', '--json', code=4)
            self.run_cli('vendors', '2604', '--json', code=3)

        def test_refresh_sync_failure_reports_catalogue_and_spend_with_or_without_cache(self):
            self.refresh_routes()
            self.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
            fixture.scripts = {1: {'status': 503, 'body': {'echo': 'secret-fixture'}}}
            for cached in (False, True):
                if cached:
                    self.run_cli('vendors', '2604', code=3)
                for as_json in (False, True):
                    result = self.run_cli('refresh', 'catalogue', *(['--json'] if as_json else []), code=4)
                    self.assertNotIn('secret-fixture', result.stdout + result.stderr)
                    self.assertEqual(result.stderr, '')
                    if as_json:
                        data = json.loads(result.stdout)
                        self.assertEqual(data['refreshed'], 'catalogue')
                        self.assertEqual((data['vendor_sync']['failed'], data['vendor_sync']['requests']), (True, 1))
                    else:
                        self.assertIn('catalogue refreshed', result.stdout)
                        self.assertIn('ForeverDB sync: failed; 1 requests this refresh', result.stdout)
                        self.assertEqual(result.stdout.count('ForeverDB sync failed'), 1)
                        if not cached:
                            self.assertNotIn('retaining', result.stdout)

        def test_malformed_http_sync_finalizes_attempt_and_spend(self):
            self.refresh_routes()
            self.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
            cache = Path(self.tmp.name, 'vendors/foreverdb.net')
            for label, response in module.MALFORMED_HTTP.items():
                for midflight in (False, True):
                    for as_json in (False, True):
                        with self.subTest(response=label, midflight=midflight, json=as_json):
                            for path in cache.glob('*.json'):
                                path.unlink()
                            count = 2 if midflight else 1
                            self.env[CAP] = str(count)
                            fixture.scripts = {count: {'body': response}}
                            result = self.run_cli('refresh', 'catalogue', *(['--json'] if as_json else []), code=4)
                            self.assertEqual(result.stderr, '')
                            self.assertNotIn('Traceback', result.stdout + result.stderr)
                            self.assertNotIn('secret-fixture', result.stdout + result.stderr)
                            if as_json:
                                data = json.loads(result.stdout)
                                self.assertEqual(data['refreshed'], 'catalogue')
                                self.assertEqual(data['vendor_sync'], {
                                    'configured': True, 'complete': False, 'pending': True,
                                    'requests': count, 'total_requests': count, 'failed': True,
                                    'deferred': None, 'status': None, 'error': 'ForeverDB sync failed'})
                            else:
                                self.assertIn('catalogue refreshed', result.stdout)
                                self.assertIn(f'ForeverDB sync: failed; {count} requests this refresh; ForeverDB sync failed', result.stdout)
                            attempt = json.loads((cache / 'attempt.json').read_text())
                            self.assertTrue(attempt['failed'])
                            self.assertIsNone(attempt['status'])
                            self.assertEqual(attempt['error'], 'ForeverDB sync failed')
                            self.assertEqual(attempt['requests'], count)
                            self.assertIsNotNone(datetime.datetime.fromisoformat(attempt['finished_at']))
                            self.assertEqual(attempt['spends'], [{'requests': count, 'finished_at': attempt['finished_at']}])
                            self.assertTrue((cache / 'pending.json').exists())
                            self.assertFalse((cache / 'index.json').exists())
                            fixture.scripts = {}
                            owner.requests.clear()
                            deferred = self.run_cli('refresh', 'catalogue', '--json', code=4)
                            sync = json.loads(deferred.stdout)['vendor_sync']
                            self.assertEqual((sync['requests'], sync['total_requests'], sync['deferred']),
                                             (0, count, 'daily sync limit'))
                            self.assertFalse(any(p.startswith('/api/v1/') for p in owner.requests))

    case = Replay('test_' + name)
    case.server = fixture.server
    result = unittest.TestResult()
    engine.start_scenario()
    try:
        case.run(result)
        for _, error in result.errors + result.failures:
            engine.errors.append('cli/' + name + ': original assertion or harness error\n' + error)
    finally:
        fixture.close()


class DBScenario:
    def __init__(self, engine, module, name):
        self.engine, self.name = engine, name
        self.fixture = Fixture(module.Handler)
        self.case = module.CLI('runTest')
        self.case.server = self.fixture.server
        self.case.setUp()
        self.case.refresh_routes()
        self.case.market()
        module.Handler.routes['/'] = (200, {})
        self.env = self.case.env
        self.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
        self.root = Path(self.case.tmp.name)
        self.cache = self.root / 'vendors/foreverdb.net'
        self.count = 0
        self.stub = self.root.parent / (self.root.name + '-credentials')
        self.stub.mkdir()
        self.env['EVERLEDGER_DYNAMIC_CREDENTIALS_DIR'] = str(self.stub)
        # Time runs: each invocation starts where the virtual time of the last one ended.
        self.engine.start_scenario(flows=True)

    def step(self, *args, **kwargs):
        self.count += 1
        return self.engine.step('foreverdb/' + self.name + '/' + str(self.count), args,
                                self.env, self.fixture, self.root, **kwargs)

    def sync(self, **kwargs):
        return self.step('refresh', 'catalogue', '--json', **kwargs)

    def lookup(self, item=2604):
        return self.step('vendors', str(item), '--json')

    def status(self):
        return self.step('status', '--json')

    def tomorrow(self):
        self.engine.advance(days=1)

    def advance(self, **delta):
        self.engine.advance(**delta)

    def journal(self):
        return json.loads((self.cache / 'attempt.json').read_text())

    def clock_to(self, stamp, **delta):
        """Start the next invocation at a journal timestamp, moved by delta."""
        self.engine.clock = datetime.datetime.fromisoformat(stamp.replace('Z', '+00:00')) + datetime.timedelta(**delta)

    def connector(self, code=None):
        self.env['EVERLEDGER_FOREVERDB_API_KEY'] = ''
        self.env['EVERLEDGER_FOREVERDB_VIA_CONNECTOR'] = 'custom.foreverdb'
        if code is None:
            code = """def add_surrogate_to_request(request, slug, allowed_hosts):
    assert slug == 'custom.foreverdb'
    assert allowed_hosts == ['foreverdb.net']
    assert request.get_header('X-api-key') is None
    request.add_header('X-Surrogate-Test', 'fixture-only')
    return request
"""
        (self.stub / 'dynamic_credentials.py').write_text(code)

    def close(self):
        self.fixture.close()
        self.case.doCleanups()
        shutil.rmtree(self.stub)


def replay_db(engine, module, name):
    s = DBScenario(engine, module, name)
    f = s.fixture
    try:
        if name == 'sync_and_sale_labels_and_locations':
            s.sync()
            for item in (2604, 2488, 9001):
                s.lookup(item)
        elif name == 'no_index_record_and_known_empty_after_removal':
            s.sync(); s.lookup(999999)
            f.forever['details']['8681']['data']['sells'].pop()
            s.tomorrow(); s.sync(); s.lookup(9001); s.lookup()
        elif name == 'daily_refresh_refetches_details_without_revision_guarantee':
            s.env[CAP] = '6'
            s.sync(); s.sync(); s.tomorrow(); s.sync()
        elif name == 'interrupted_sync_preserves_previous_index':
            s.sync(); s.tomorrow()
            f.scripts = {1: {'status': 503, 'body': {}}}
            s.sync(); s.lookup()
        elif name == 'budgeted_bootstrap_resumes_without_activating_partial_index':
            f.large(); s.sync(); s.status(); s.sync(); s.tomorrow(); s.sync(); s.lookup()
        elif name == 'mid_detail_interrupt_resumes_and_old_index_survives':
            s.sync(); s.tomorrow()
            f.scripts = {3: {'hold': True}}
            s.sync(death=signal.SIGINT, policy='python-internal')
            s.lookup(); s.tomorrow(); f.scripts = {}; s.sync(); s.lookup()
        elif name == 'unknown_labels_and_malformed_shapes_do_not_invent_provenance':
            del f.forever['details']['8681']['data']['sells'][0]['seenInForever']
            s.sync(); s.lookup(); s.tomorrow()
            f.forever['pages'][0]['data'][0].pop('id')
            s.sync(); s.lookup()
        elif name == 'abrupt_exit_spend_uses_the_last_request_start':
            s.env[CAP] = '3'
            f.scripts = {3: {'hold': True}}
            s.sync(death=signal.SIGKILL)
            last = s.journal()['last_request_at']
            f.scripts = {}
            # The killed attempt spent the cap, dated at its last request start.
            s.clock_to(last, days=1, seconds=-1); s.sync(); s.status()
            s.clock_to(last, days=1); s.sync()
        elif name == 'changed_extraction_discards_checkpoint_and_recovers_next_day':
            observed = json.loads((ROOT / 'tests/fixtures/foreverdb-list.json').read_text())
            observed['meta']['total'] = 101
            f.forever = {'details': {}, 'pages': [observed, {'data': [{'id': 101}], 'meta': {
                'build': 'next-build', 'total': 101, 'limit': 100, 'offset': 100, 'next': None}}]}
            s.env[CAP] = '1'
            s.sync(); s.tomorrow()
            s.env.pop(CAP)
            s.sync(); s.status(); s.sync(); s.tomorrow()
            f.forever = json.loads((ROOT / 'tests/fixtures/foreverdb.json').read_text())
            s.sync(); s.lookup()
            shutil.rmtree(s.cache)
            f.large(3601, 37)
            s.sync(); s.tomorrow()
            f.forever['pages'][36]['meta']['build'] = 'next-build'
            s.sync(); s.status(); s.sync(); s.tomorrow()
            # The restarted traversal reaches pages 36 and 37 a day later and meets the changed build again.
            s.sync(); s.tomorrow()
            f.forever = json.loads((ROOT / 'tests/fixtures/foreverdb.json').read_text())
            s.sync(); s.lookup()
        elif name == 'failed_index_stays_ineligible_through_incomplete_retry':
            s.sync()
            path = s.cache / 'attempt.json'
            data = json.loads(path.read_text()); data.update(failed=True, status=401)
            path.write_text(json.dumps(data)); s.status(); s.tomorrow()
            f.large(); s.sync(); s.status(); s.sync(); s.tomorrow(); s.sync(); s.status()
        elif name == 'invalid_header_transport_error_cannot_print_key':
            s.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture\nbad-header'
            s.sync(forbidden=('secret-fixture',))
        elif name == 'key_files_precedence_and_no_key_dormancy':
            s.env.pop('EVERLEDGER_FOREVERDB_API_KEY')
            s.sync(); s.status()
            (s.root / 'foreverdb-api-key').write_text(' default-key\n'); s.sync()
            (s.root / 'explicit-key').write_text(' file-key\n')
            s.env['EVERLEDGER_FOREVERDB_API_KEY_FILE'] = str(s.root / 'explicit-key')
            s.tomorrow(); s.sync()
            s.env['EVERLEDGER_FOREVERDB_API_KEY'] = ' env-key '
            s.tomorrow(); s.sync()
        elif name == 'pacing_and_failures_do_not_expose_credentials':
            s.sync(); s.tomorrow()
            f.scripts = {1: {'status': 503, 'body': {'echo': 'secret-fixture'}}}
            s.sync(forbidden=('secret-fixture',))
            f.scripts = {}
            s.sync()
        elif name == 'observed_list_page_uses_total_and_offset':
            f.large(1014, 11)
            f.forever['pages'][0] = json.loads((ROOT / 'tests/fixtures/foreverdb-list.json').read_text())
            template = copy.deepcopy(f.forever['details']['1']['data'])
            for row in f.forever['pages'][0]['data']:
                f.forever['details'][str(row['id'])] = {'data': dict(copy.deepcopy(template), id=row['id'])}
            for page in f.forever['pages'][1:]:
                page['meta']['build'] = f.forever['pages'][0]['meta']['build']
                for row in page['data']:
                    row['id'] += 1000000
                    f.forever['details'][str(row['id'])] = {'data': dict(copy.deepcopy(template), id=row['id'])}
            s.sync(); s.status()
        elif name == 'stack_prices_and_limited_stock_are_exact':
            sale = f.forever['details']['8681']['data']['sells'][0]
            sale.update(price=10, stack=3, pricePerItem=3.3333333333, limited=True, limitedStockCount=None)
            s.sync(); s.lookup(); s.tomorrow(); sale['price'] = 12
            s.sync(); s.lookup(); s.lookup(9001)
        elif name == '502_retries_once_after_requested_delay_and_counts_request':
            f.scripts = {1: {'status': 502, 'body': {'retryable': True, 'retry_after': 75}}}
            s.sync()
        elif name == '502_retry_at_budget_boundary_resumes_next_day':
            s.env[CAP] = '1'
            f.scripts = {1: {'status': 502, 'body': {'retryable': True, 'retry_after': 60}}}
            s.sync(); s.status(); s.tomorrow()
            s.env.pop(CAP); f.scripts = {}
            s.sync()
            shutil.rmtree(s.cache)
            f.large()
            f.scripts = {36: {'status': 502, 'body': {'retryable': True, 'retry_after': 60}}}
            s.sync(); s.status(); s.tomorrow(); f.scripts = {}; s.sync()
        elif name == '502_retry_failure_stops_after_two_requests':
            f.scripts = {i: {'status': 502, 'body': {'retryable': True, 'retry_after': 60}} for i in (1, 2)}
            s.sync(); s.status()
        elif name == 'connector_sync_uses_stub_module_and_browser_ua_without_key':
            s.connector(); s.sync(forbidden=('fixture-only',)); s.lookup(2488); s.status()
        elif name == 'env_transport_outgoing_user_agent':
            f.scripts = {1: {'body': {}}}
            s.sync()
        elif name == 'missing_connector_module_is_clear_and_status_still_works':
            s.env['EVERLEDGER_FOREVERDB_VIA_CONNECTOR'] = 'custom.foreverdb'
            s.sync(); s.status()
        elif name == 'both_transports_refuse_redirects':
            f.scripts = {1: {'status': 302, 'headers': {'Location': '/redirected'}}}
            s.sync(); s.tomorrow(); s.connector(); s.sync()
        elif name == 'connector_exceptions_are_redacted_and_journaled':
            sentinel = 'secret-fixture-surrogate'
            for code in [f"raise RuntimeError('{sentinel}')\n",
                         f"def add_surrogate_to_request(*args, **kwargs):\n    raise RuntimeError('{sentinel}')\n",
                         f"import sys\nsys.path.insert(0, {str(ROOT / 'python')!r})\nfrom everledger_core import Failure\ndef add_surrogate_to_request(*args, **kwargs):\n    raise Failure('{sentinel}', 4, {{'response': '{sentinel}'}})\n"]:
                s.connector(code)
                (s.cache / 'attempt.json').unlink(missing_ok=True)
                s.sync(forbidden=(sentinel,)); s.status()
        elif name == 'unset_daily_requests_defaults_to_36':
            f.large(); s.env.pop(CAP, None)
            s.sync(); s.status()
        elif name == 'valid_daily_requests_is_parsed_once':
            f.large(); s.env[CAP] = '1'
            s.sync(); s.status(); s.tomorrow()
            s.env[CAP] = '5000'
            s.sync(); s.lookup()
        elif name == 'invalid_daily_requests_is_a_usage_error':
            for value in ('', 'invalid', '1.5', '0', '-1'):
                s.env[CAP] = value
                s.sync(); s.status(); s.lookup()
        elif name == 'sync_honors_raised_daily_cap_from_environment':
            f.large(); s.env[CAP] = '40'
            s.sync(); s.status()
            shutil.rmtree(s.cache)
            s.env[CAP] = '5000'
            s.sync(); s.status(); s.lookup()
        elif name == 'window_spend_across_two_attempts_defers_at_cap':
            s.env[CAP] = '12'
            s.sync(); s.advance(minutes=34); s.sync(); s.sync(); s.status()
        elif name == 'raised_cap_resumes_inside_window_with_remaining_budget':
            s.env[CAP] = '2'
            s.sync(); s.status(); s.advance(minutes=34)
            s.env[CAP] = '6'
            s.sync(); s.lookup()
        elif name == 'window_expiry_restores_only_expired_spend_then_full_budget':
            s.env[CAP] = '4'
            s.sync()
            first = s.journal()['finished_at']
            s.advance(hours=1)
            s.env[CAP] = '6'
            s.sync()
            second = s.journal()['finished_at']
            s.clock_to(first, days=1, seconds=-1); s.sync()   # both spends are inside the window
            s.clock_to(first, days=1); s.sync()               # the first has expired
            s.clock_to(second, days=1); s.sync()              # and now the second
            s.tomorrow(); s.sync(); s.status()
        elif name == 'index_omits_checkpoint_payload_and_ignores_legacy_fields':
            s.sync(); s.lookup()
            path = s.cache / 'index.json'
            index = json.loads(path.read_text())
            index.update(details={'obsolete': 'ignored'}, sync_requests=-1)
            path.write_text(json.dumps(index))
            s.lookup(); s.status(); s.tomorrow(); s.sync(); s.lookup()
        elif name == 'sync_result_shape_on_success_failure_deferred_and_unconfigured':
            key = s.env.pop('EVERLEDGER_FOREVERDB_API_KEY')
            s.sync(); s.status()
            s.env['EVERLEDGER_FOREVERDB_API_KEY'] = key
            f.scripts = {1: {'status': 503, 'body': {'secret': 'private upstream'}}}
            s.sync(forbidden=('private upstream',))
            f.scripts = {}
            s.env[CAP] = '1'
            s.sync()
            s.env.pop(CAP)
            s.sync(); s.status()
        elif name == 'refresh_cached_returns_setup_failure_without_raising':
            s.env['EVERLEDGER_FOREVERDB_API_KEY'] = ''
            s.env['EVERLEDGER_FOREVERDB_API_KEY_FILE'] = str(s.root)
            s.sync(); s.status(); s.lookup()
        elif name == 'sync_checkpoint_write_failure_preserves_request_count':
            page = f.forever['pages'][0]
            for blocked in ('pending.json', 'attempt.json'):
                shutil.rmtree(s.cache, ignore_errors=True)

                def block(blocked=blocked):
                    path = s.cache / blocked
                    path.unlink(missing_ok=True)
                    path.mkdir()
                f.scripts = {1: {'hold': True, 'body': page}}
                s.sync(interfere=block)
                f.scripts = {}
                s.status(); s.sync()
        elif name == '502_long_wait_is_pending_and_resumes_after_not_before':
            f.scripts = {1: {'status': 502, 'body': {'retryable': True, 'retry_after': 301}}}
            s.sync(); s.status()
            f.scripts = {}
            s.sync()
            s.advance(seconds=302)
            s.sync(); s.lookup()
        elif name == '502_numeric_delays_never_overflow_or_sleep_over_300':
            for delay in ('0', '60', '299.9', '300', '301', '1e15', '1' + '0' * 1000, 'Infinity', '-Infinity', 'NaN'):
                shutil.rmtree(s.cache, ignore_errors=True)
                f.scripts = {1: {'status': 502, 'body': '{"retryable": true, "retry_after": %s}' % delay}}
                s.sync(); s.status()
        elif name == 'abrupt_exit_preserves_prior_window_spend':
            s.env[CAP] = '8'
            s.sync()
            f.scripts = {1: {'hold': True}}
            s.sync(death=signal.SIGKILL)
            f.scripts = {}
            s.sync(); s.sync(); s.status()
        elif name == '502_retry_counts_against_remaining_window_budget':
            s.env[CAP] = '1'
            s.sync()
            s.env[CAP] = '2'
            f.scripts = {1: {'status': 502, 'body': {'retryable': True}}}
            s.sync(); s.sync(); s.status()
        else:
            raise ValueError('missing scenario: ' + name)
    finally:
        s.close()
