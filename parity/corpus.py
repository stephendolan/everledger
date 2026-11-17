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


ORACLE_COMMIT = 'd28214c'
CAP = 'EVERLEDGER_FOREVERDB_DAILY_REQUESTS'
_reference = None


def reference_root():
    """A directory holding the README the oracle's help is checked against.

    The working tree's README documents the Rust binary. The replayed test
    compares the oracle's own help with the README of the oracle's commit.
    """
    global _reference
    if _reference is None:
        _reference = tempfile.TemporaryDirectory(prefix='everledger-parity-reference-')
        readme = subprocess.run(['git', 'show', ORACLE_COMMIT + ':README.md'], cwd=ROOT,
                                capture_output=True, check=True).stdout
        Path(_reference.name, 'README.md').write_bytes(readme)
    return Path(_reference.name)


def load_cli():
    spec = importlib.util.spec_from_file_location('parity_reference_cli', ROOT / 'tests/test_cli.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.NOW = ORIGIN
    # The fixture directory is already resolved; only the README check reads ROOT now.
    module.ROOT = reference_root()
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
    'legacy_checkpoint_migrates_own_requests_at_finish_or_last_request',
    'abrupt_exit_preserves_prior_window_spend', '502_retry_counts_against_remaining_window_budget',
]
TRANSLATIONS = {
    'cache_failure_window': 'The two in-process lookup(refresh=True) calls become refresh catalogue, followed by vendors; the same two-day and eight-day cache states and 503 response are retained.',
    'sync_foreverdb': 'The in-process transport patch becomes refresh catalogue against real fixture HTTP with the virtual clock.',
    'abrupt_exit_spend_uses_the_last_request_start': 'SIGKILL at the third held vendor request, at a cap of 3. The next invocation starts one second short of 24 hours after that request began, when attempted_at is already more than 24 hours old, and is deferred; one second later the spend has expired. (The original moves its clock five minutes per request and checks 23h55m.)',
    'changed_extraction_discards_checkpoint_and_recovers_next_day': 'A cap of 1 reads one list page; the next day the second page carries another build, twice. Then, on a cleared cache, 36 list pages spend the default cap before the changed build appears on page 37, and the restarted traversal meets it again a day later.',
    '502_retry_at_budget_boundary_resumes_next_day': 'A cap of 1 whose only request is a retryable 502. Then, on a cleared cache, the same boundary at the default cap: the list request and 35 details, with the 502 as request 36.',
    'daily_refresh_refetches_details_without_revision_guarantee': 'The cap of 6 is set through the variable.',
    'pacing_and_failures_do_not_expose_credentials': 'The scripted 503 is removed before the last refresh, which the original runs on its restored transport.',
    'window_spend_across_two_attempts_defers_at_cap': 'A cap of 12 through the variable; the scenario clock moves 34 minutes between the two attempts.',
    'raised_cap_resumes_inside_window_with_remaining_budget': 'Caps of 2 and then 6 through the variable, 34 minutes apart on the scenario clock.',
    'window_expiry_restores_only_expired_spend_then_full_budget': 'The original holds its clock still during an attempt; here an attempt takes virtual time, so each boundary is taken from the journal: one second before the first spend is 24 hours old, that second, and the second on which the next spend expires.',
    'legacy_checkpoint_migrates_own_requests_at_finish_or_last_request': 'The legacy journal is written into the cache directory with timestamps relative to the scenario clock.',
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
    # Detect both additions and removals, rather than silently printing 82.
    import ast
    tree = ast.parse((ROOT / 'tests/test_foreverdb.py').read_text())
    owners = {node.name[5:]: owner.name for owner in tree.body if isinstance(owner, ast.ClassDef)
              for node in owner.body if isinstance(node, ast.FunctionDef) and node.name.startswith('test_')}
    if set(owners) != set(INTERNAL) or len(rows) + len(INTERNAL) != 82:
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
                         f"from everledger_core import Failure\ndef add_surrogate_to_request(*args, **kwargs):\n    raise Failure('{sentinel}', 4, {{'response': '{sentinel}'}})\n"]:
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
        elif name == 'legacy_checkpoint_migrates_own_requests_at_finish_or_last_request':
            for timestamp in ('finished_at', 'last_request_at'):
                stamp = lambda **ago: (s.engine.now() - datetime.timedelta(**ago)).isoformat(timespec='seconds').replace('+00:00', 'Z')
                for stale in ('pending.json', 'index.json'):
                    (s.cache / stale).unlink(missing_ok=True)
                s.cache.mkdir(parents=True, exist_ok=True)
                (s.cache / 'attempt.json').write_text(json.dumps({
                    'attempted_at': stamp(days=2), timestamp: stamp(minutes=34), 'requests': 2,
                    'total_requests': 100, 'sync_id': 'legacy', 'failed': False}))
                s.env[CAP] = '2'
                s.sync()
                s.env[CAP] = '6'
                s.sync(); s.sync(); s.status()
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
