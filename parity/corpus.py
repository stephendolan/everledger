"""Replay the oracle tests; explicit CLI translations of in-process contracts."""
import copy
import datetime
import importlib.util
import json
from pathlib import Path
import signal
import subprocess
import tempfile
import types
import unittest

from parity.engine import ORIGIN, ROOT
from parity.fixture import Fixture


ORACLE_COMMIT = 'b24e183'
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
    'unknown_labels_and_malformed_shapes_do_not_invent_provenance', 'abrupt_exit_cooldown_uses_the_last_request_start',
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
]
TRANSLATIONS = {
    'cache_failure_window': 'The two in-process lookup(refresh=True) calls become refresh catalogue, followed by vendors; the same two-day and eight-day cache states and 503 response are retained.',
    'sync_foreverdb': 'The in-process transport patch becomes refresh catalogue against real fixture HTTP with the virtual clock.',
    'abrupt_exit_cooldown_uses_the_last_request_start': 'SIGKILL at the third held vendor request; the next invocation starts 24h05m after attempted_at and 23h55m after the last request start (the original transport advanced wall time by five minutes per successful request).',
    'changed_extraction_discards_checkpoint_and_recovers_next_day': '36 list pages consume the real cap before the changed extraction is observed on page 37.',
    '502_retry_at_budget_boundary_resumes_next_day': '35 successful detail requests plus the list request reach the real cap; the retryable 502 occupies request 36.',
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
    # Detect both additions and removals, rather than silently printing 76.
    import ast
    tree = ast.parse((ROOT / 'tests/test_foreverdb.py').read_text())
    owners = {node.name[5:]: owner.name for owner in tree.body if isinstance(owner, ast.ClassDef)
              for node in owner.body if isinstance(node, ast.FunctionDef) and node.name.startswith('test_')}
    if set(owners) != set(INTERNAL) or len(rows) + len(INTERNAL) != 76:
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
        self.engine.start_scenario()

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
        path = self.cache / 'attempt.json'
        state = json.loads(path.read_text())
        state.update(attempted_at='2020-01-01T00:00:00Z', finished_at='2020-01-01T00:00:00Z')
        path.write_text(json.dumps(state))

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
        import shutil
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
        elif name == 'abrupt_exit_cooldown_uses_the_last_request_start':
            f.scripts = {3: {'hold': True}}
            s.sync(death=signal.SIGKILL)
            state = json.loads((s.cache / 'attempt.json').read_text())
            # The third start is 12:00:04Z; 23h55m later is next day 11:55:04Z.
            # Move the journal back by 23h55m while keeping the process origin.
            origin = datetime.datetime.fromisoformat(ORIGIN.replace('Z', '+00:00'))
            shifted = origin - datetime.timedelta(hours=23, minutes=55)
            state['last_request_at'] = shifted.isoformat(timespec='seconds').replace('+00:00', 'Z')
            state['attempted_at'] = (origin - datetime.timedelta(hours=24, minutes=5)).isoformat(timespec='seconds').replace('+00:00', 'Z')
            (s.cache / 'attempt.json').write_text(json.dumps(state))
            f.scripts = {}; s.sync(); s.status()
        elif name == 'changed_extraction_discards_checkpoint_and_recovers_next_day':
            f.large(3601, 37)
            s.sync(); s.tomorrow()
            f.forever['pages'][36]['meta']['build'] = 'next-build'
            s.sync(); s.status(); s.sync(); s.tomorrow()
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
            s.sync(forbidden=('secret-fixture',)); s.sync()
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
            import shutil
            shutil.rmtree(s.cache)
            s.env[CAP] = '5000'
            s.sync(); s.status(); s.lookup()
        else:
            raise ValueError('missing scenario: ' + name)
    finally:
        s.close()
