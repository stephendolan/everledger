"""No network: observed wire shapes with explicit synthetic regression cases."""
import datetime as dt
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import everledger_core as core
import everledger_foreverdb as db

FIXTURE = Path(__file__).parent / 'fixtures/foreverdb.json'


def configured_db():
    spec = importlib.util.spec_from_file_location('configured_foreverdb', db.__file__)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ForeverDBConfiguration(unittest.TestCase):
    def test_unset_daily_requests_defaults_to_36(self):
        with patch.dict('os.environ', {}, clear=True):
            self.assertEqual(configured_db().DAILY_REQUESTS, 36)

    def test_valid_daily_requests_is_parsed_once(self):
        for value in ('1', '5000'):
            with self.subTest(value=value), patch.dict('os.environ', {
                    'EVERLEDGER_FOREVERDB_DAILY_REQUESTS': value}):
                configured = configured_db()
                with patch.dict('os.environ', {'EVERLEDGER_FOREVERDB_DAILY_REQUESTS': 'invalid'}):
                    self.assertEqual(configured.DAILY_REQUESTS, int(value))

    def test_invalid_daily_requests_is_a_usage_error(self):
        for value in ('', 'invalid', '1.5', '0', '-1'):
            with self.subTest(value=value), patch.dict('os.environ', {
                    'EVERLEDGER_FOREVERDB_DAILY_REQUESTS': value}):
                with self.assertRaisesRegex(core.Failure, 'EVERLEDGER_FOREVERDB_DAILY_REQUESTS') as error:
                    configured_db()
                self.assertEqual(error.exception.code, 2)


class ForeverDB(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)
        self.fixture = json.loads(FIXTURE.read_text())
        self.requests = []
        self.clock = [core.now()]
        for module in (core, db):
            clock = patch.object(module, 'now', side_effect=lambda: self.clock[0])
            clock.start()
            self.addCleanup(clock.stop)
        self.key = patch.dict('os.environ', {'EVERLEDGER_FOREVERDB_API_KEY': 'secret-fixture',
            'EVERLEDGER_FOREVERDB_VIA_CONNECTOR': '', 'EVERLEDGER_DYNAMIC_CREDENTIALS_DIR': str(self.directory)})
        self.key.start()
        self.addCleanup(self.key.stop)
        self.paths = patch.multiple(db, CACHE=self.directory, DATA_DIR=self.directory)
        self.paths.start()
        self.addCleanup(self.paths.stop)
        import everledger_vendors as vendors
        ninja_path = patch.object(vendors, 'CACHE', self.directory / 'ninja')
        ninja_path.start()
        self.addCleanup(ninja_path.stop)
        self.transport = patch.object(db, 'request', side_effect=self.get)
        self.transport.start()
        self.addCleanup(self.transport.stop)
        self.sleep = patch.object(db.time, 'sleep')
        self.sleep_mock = self.sleep.start()
        self.addCleanup(self.sleep.stop)

    def get(self, url, headers):
        from urllib.parse import urlsplit, parse_qs
        self.requests.append(url)
        self.assertEqual(headers['X-API-Key'], 'secret-fixture')
        self.assertTrue(headers['User-Agent'].startswith('Mozilla/5.0'))
        parsed = urlsplit(url)
        if parsed.path == '/api/v1/vendors':
            query = parse_qs(parsed.query)
            self.assertEqual(query['limit'], ['100'])
            offset = int(query['offset'][0])
            data = self.fixture['pages'][offset // 100]
        else:
            data = self.fixture['details'][parsed.path.rsplit('/', 1)[1]]
        return json.dumps(data).encode(), {}

    def tomorrow(self):
        self.clock[0] += dt.timedelta(days=1)

    def test_sync_and_sale_labels_and_locations(self):
        result = db.sync()
        self.assertTrue(result['complete'])
        self.assertEqual(result['requests'], 6)
        data = db.lookup(2604)
        self.assertEqual({v['name'] for v in data['vendors']}, {
            'Poranna Snowbraid', 'Outfitter Eric', 'Bombus Finespindle'})
        rows = {v['name']: v for v in data['vendors']}
        self.assertEqual(rows['Outfitter Eric']['basis'], 'unverified')
        self.assertEqual(rows['Outfitter Eric']['vendor_basis'], 'seen')
        self.assertEqual(rows['Bombus Finespindle']['basis'], 'seen')
        self.assertEqual(rows['Outfitter Eric']['coords'], [[43.4, 29.3]])
        self.assertEqual(rows['Outfitter Eric']['locations'][0]['mapId'], 87)
        self.assertEqual(rows['Outfitter Eric']['side'], 'Alliance')
        self.assertEqual(db.lookup(2488)['vendors'][0]['price_copper'], 536)
        self.assertEqual(db.lookup(2488)['vendors'][0]['basis'], 'seen')
        self.assertEqual(db.lookup(9001)['vendors'][0]['basis'], 'baseline')
        import everledger_vendors as vendors
        self.assertIn('sale/price Classic-only baseline', '\n'.join(vendors.lines(db.lookup(9001))))
        self.assertEqual(rows['Outfitter Eric']['price_copper'], 50)
        self.assertIn('Data from ForeverDB.net', data['sources'][0]['detail'])
        self.assertIn('https://foreverdb.net', data['sources'][0]['detail'])

    def test_no_index_record_and_known_empty_after_removal(self):
        db.sync()
        self.assertIsNone(db.lookup(999999)['vendors'])
        self.assertEqual(db.lookup(999999)['reason'], 'item not in dataset')
        self.fixture['details']['8681']['data']['sells'].pop()
        self.tomorrow()
        result = db.sync()
        self.assertEqual(result['requests'], 6)
        self.assertEqual(db.lookup(9001)['vendors'], [])
        self.assertIsNone(db.lookup(9001)['reason'])
        self.assertEqual(len(db.lookup(2604)['vendors']), 3)

    @patch.object(db, 'DAILY_REQUESTS', 6)
    def test_daily_refresh_refetches_details_without_revision_guarantee(self):
        db.sync()
        self.requests.clear()
        self.assertEqual(db.sync()['requests'], 0)
        self.assertEqual(self.requests, [])
        self.tomorrow()
        result = db.sync()
        self.assertEqual(result['requests'], 6)
        self.assertEqual(len(self.requests), 6)

    def test_interrupted_sync_preserves_previous_index(self):
        db.sync()
        before = (self.directory / 'index.json').read_bytes()
        self.tomorrow()
        with patch.object(db, 'request', side_effect=core.Failure('fixture failure', 4)):
            with self.assertRaises(core.Failure):
                db.sync()
        self.assertEqual((self.directory / 'index.json').read_bytes(), before)
        self.assertEqual(len(db.lookup(2604)['vendors']), 3)

    def test_budgeted_bootstrap_resumes_without_activating_partial_index(self):
        template = self.fixture['details']['8681']['data']
        self.fixture['pages'] = [{'data': [dict(template, id=n) for n in range(1, 41)],
                                  'meta': {'build': 'fixture', 'total': 40, 'limit': 100, 'offset': 0, 'next': None}}]
        self.fixture['details'] = {str(n): {'data': dict(template, id=n)} for n in range(1, 41)}
        first = db.sync()
        self.assertEqual(first['requests'], 36)
        self.assertFalse(first['complete'])
        self.assertIsNone(db.index())
        self.assertEqual(db.sync()['requests'], 0)
        self.tomorrow()
        resumed = db.sync()
        self.assertEqual(resumed['requests'], 5)
        self.assertEqual(resumed['total_requests'], 41)
        self.assertEqual(len(db.lookup(2604)['vendors']), 40)

    def test_sync_honors_raised_daily_cap_from_environment(self):
        template = self.fixture['details']['8681']['data']
        self.fixture['pages'] = [{'data': [dict(template, id=n) for n in range(1, 41)],
                                  'meta': {'build': 'fixture', 'total': 40, 'limit': 100, 'offset': 0, 'next': None}}]
        self.fixture['details'] = {str(n): {'data': dict(template, id=n)} for n in range(1, 41)}
        for cap, requests, complete in ((40, 40, False), (5000, 41, True)):
            with self.subTest(cap=cap), patch.dict('os.environ', {
                    'EVERLEDGER_FOREVERDB_DAILY_REQUESTS': str(cap)}):
                configured = configured_db()
                self.requests.clear()
                with patch.multiple(configured, CACHE=self.directory / str(cap), request=self.get):
                    result = configured.sync()
                    self.assertEqual(result['requests'], requests)
                    self.assertEqual(len(self.requests), requests)
                    self.assertEqual(result['complete'], complete)
                    self.assertEqual(configured.index() is not None, complete)

    @patch.object(db, 'DAILY_REQUESTS', 12)
    def test_window_spend_across_two_attempts_defers_at_cap(self):
        self.assertEqual(db.sync()['requests'], 6)
        first_finish = core.iso(self.clock[0])
        self.clock[0] += dt.timedelta(minutes=34)
        self.assertEqual(db.sync()['requests'], 6)
        checkpoint = json.loads((self.directory / 'attempt.json').read_text())
        self.assertEqual(checkpoint['spends'], [
            {'requests': 6, 'finished_at': first_finish},
            {'requests': 6, 'finished_at': core.iso(self.clock[0])}])
        before = len(self.requests)
        deferred = db.sync()
        self.assertEqual(deferred, {'configured': True, 'complete': True,
            'pending': False, 'requests': 0, 'deferred': 'daily sync limit', 'failed': False})
        self.assertEqual(len(self.requests), before)

    def test_raised_cap_resumes_inside_window_with_remaining_budget(self):
        with patch.object(db, 'DAILY_REQUESTS', 2):
            first = db.sync()
        self.assertEqual(first['requests'], 2)
        self.assertFalse(first['complete'])
        self.assertIsNone(db.index())
        self.clock[0] += dt.timedelta(minutes=34)
        with patch.object(db, 'DAILY_REQUESTS', 6):
            resumed = db.sync()
        self.assertEqual(resumed, {'configured': True, 'complete': True,
            'pending': False, 'requests': 4, 'total_requests': 6})
        self.assertEqual(len(self.requests), 6)
        self.assertEqual(len(db.lookup(2604)['vendors']), 3)

    def test_window_expiry_restores_only_expired_spend_then_full_budget(self):
        start = self.clock[0]
        with patch.object(db, 'DAILY_REQUESTS', 4):
            self.assertEqual(db.sync()['requests'], 4)
        self.clock[0] += dt.timedelta(hours=1)
        with patch.object(db, 'DAILY_REQUESTS', 6):
            self.assertEqual(db.sync()['requests'], 2)
            self.clock[0] = start + dt.timedelta(days=1, seconds=-1)
            self.assertEqual(db.sync()['requests'], 0)
            self.clock[0] += dt.timedelta(seconds=1)
            partial = db.sync()
            self.assertEqual(partial['requests'], 4)
            self.assertFalse(partial['complete'])
            checkpoint = json.loads((self.directory / 'attempt.json').read_text())
            self.assertEqual([spend['requests'] for spend in checkpoint['spends']], [2, 4])
            self.clock[0] += dt.timedelta(hours=1)
            self.assertEqual(db.sync()['requests'], 2)
            self.tomorrow()
            full = db.sync()
            self.assertTrue(full['complete'])
            self.assertEqual(full['requests'], 6)
            checkpoint = json.loads((self.directory / 'attempt.json').read_text())
            self.assertEqual(checkpoint['spends'], [
                {'requests': 6, 'finished_at': core.iso(self.clock[0])}])

    def test_legacy_checkpoint_migrates_own_requests_at_finish_or_last_request(self):
        for timestamp in ('finished_at', 'last_request_at'):
            with self.subTest(timestamp=timestamp):
                (self.directory / 'pending.json').unlink(missing_ok=True)
                (self.directory / 'index.json').unlink(missing_ok=True)
                self.requests.clear()
                finished = core.iso(self.clock[0] - dt.timedelta(minutes=34))
                (self.directory / 'attempt.json').write_text(json.dumps({
                    'attempted_at': core.iso(self.clock[0] - dt.timedelta(days=2)),
                    timestamp: finished, 'requests': 2, 'total_requests': 100,
                    'sync_id': 'legacy', 'failed': False}))
                with patch.object(db, 'DAILY_REQUESTS', 2):
                    self.assertEqual(db.sync()['requests'], 0)
                    self.assertEqual(self.requests, [])
                with patch.object(db, 'DAILY_REQUESTS', 6):
                    resumed = db.sync()
                    self.assertEqual(resumed['requests'], 4)
                    self.assertFalse(resumed['complete'])
                    checkpoint = json.loads((self.directory / 'attempt.json').read_text())
                    self.assertEqual(checkpoint['spends'][0], {'requests': 2, 'finished_at': finished})
                    self.assertEqual(db.sync()['requests'], 0)

    def test_mid_detail_interrupt_resumes_and_old_index_survives(self):
        db.sync()
        before = (self.directory / 'index.json').read_bytes()
        self.tomorrow()
        original = self.get

        def interrupt(url, headers):
            if url.endswith('/5154'):
                raise KeyboardInterrupt()
            return original(url, headers)

        with patch.object(db, 'request', side_effect=interrupt):
            with self.assertRaises(KeyboardInterrupt):
                db.sync()
        self.assertEqual((self.directory / 'index.json').read_bytes(), before)
        self.tomorrow()
        self.requests.clear()
        result = db.sync()
        self.assertTrue(result['complete'])
        self.assertEqual(result['requests'], 4)
        self.assertEqual(len(db.lookup(2604)['vendors']), 3)

    def test_unknown_labels_and_malformed_shapes_do_not_invent_provenance(self):
        del self.fixture['details']['8681']['data']['sells'][0]['seenInForever']
        db.sync()
        row = next(row for row in db.lookup(2604)['vendors'] if row['npc_id'] == 8681)
        self.assertEqual(row['basis'], 'unverified')
        self.tomorrow()
        self.fixture['pages'][0]['data'][0].pop('id')
        with self.assertRaises(core.Failure):
            db.sync()
        self.assertEqual(len(db.lookup(2604)['vendors']), 3)

    @patch.object(db, 'DAILY_REQUESTS', 3)
    def test_abrupt_exit_spend_uses_the_last_request_start(self):
        import datetime
        clock = [core.now()]
        original = self.get
        count = [0]

        def crash(url, headers):
            count[0] += 1
            if count[0] == 3:
                raise SystemExit('simulate abrupt process exit')
            data = original(url, headers)
            clock[0] += datetime.timedelta(minutes=5)
            return data

        with patch.object(core, 'now', side_effect=lambda: clock[0]), \
                patch.object(db, 'now', side_effect=lambda: clock[0]):
            with patch.object(db, 'request', side_effect=crash):
                with self.assertRaises(SystemExit):
                    db.sync()
            clock[0] += datetime.timedelta(hours=23, minutes=55)
            before = len(self.requests)
            self.assertEqual(db.sync()['requests'], 0)
            self.assertEqual(len(self.requests), before)

    @patch.object(db, 'DAILY_REQUESTS', 8)
    def test_abrupt_exit_preserves_prior_window_spend(self):
        self.assertEqual(db.sync()['requests'], 6)
        with patch.object(db, 'request', side_effect=SystemExit('abrupt exit')):
            with self.assertRaises(SystemExit):
                db.sync()
        resumed = db.sync()
        self.assertEqual(resumed['requests'], 1)
        self.assertFalse(resumed['complete'])
        checkpoint = json.loads((self.directory / 'attempt.json').read_text())
        self.assertEqual([spend['requests'] for spend in checkpoint['spends']], [6, 1, 1])
        self.assertEqual(db.sync()['requests'], 0)

    def test_changed_extraction_discards_checkpoint_and_recovers_next_day(self):
        first = json.loads((FIXTURE.parent / 'foreverdb-list.json').read_text())
        first['meta']['total'] = 101
        self.fixture['pages'] = [first, {'data': [{'id': 101}], 'meta': {
            'build': 'next-build', 'total': 101, 'limit': 100, 'offset': 100, 'next': None}}]
        with patch.object(db, 'DAILY_REQUESTS', 1):
            self.assertFalse(db.sync()['complete'])
        self.tomorrow()
        self.fixture['pages'][1]['meta']['build'] = 'next-build'
        with self.assertRaises(core.Failure):
            db.sync()
        self.assertIsNone(db.index())
        self.assertFalse((self.directory / 'pending.json').exists())
        with self.assertRaises(core.Failure):
            db.sync()
        self.tomorrow()
        self.fixture = json.loads(FIXTURE.read_text())
        self.assertTrue(db.sync()['complete'])
        self.assertEqual(len(db.lookup(2604)['vendors']), 3)

    def test_failed_index_stays_ineligible_through_incomplete_retry(self):
        import everledger_vendors as vendors
        db.sync()
        attempt = json.loads((self.directory / 'attempt.json').read_text())
        attempt.update(failed=True, status=401)
        (self.directory / 'attempt.json').write_text(json.dumps(attempt))
        self.assertIsNone(vendors.active_index())
        self.tomorrow()
        template = self.fixture['details']['8681']['data']
        self.fixture['pages'] = [{'data': [dict(template, id=n) for n in range(1, 41)],
                                  'meta': {'build': 'fixture', 'total': 40, 'limit': 100, 'offset': 0, 'next': None}}]
        self.fixture['details'] = {str(n): {'data': dict(template, id=n)} for n in range(1, 41)}
        self.assertFalse(db.sync()['complete'])
        self.assertIsNone(vendors.active_index())
        deferred = db.sync()
        self.assertFalse(deferred['complete'])
        self.assertTrue(deferred['pending'])
        self.tomorrow()
        self.assertTrue(db.sync()['complete'])
        self.assertIsNotNone(vendors.active_index())

    def test_invalid_header_transport_error_cannot_print_key(self):
        with patch.object(db, 'request', side_effect=ValueError('invalid header secret-fixture')):
            with self.assertRaises(core.Failure) as error:
                db.sync()
        self.assertNotIn('secret-fixture', str(error.exception))
        self.assertTrue(json.loads((self.directory / 'attempt.json').read_text())['failed'])

    def test_key_files_precedence_and_no_key_dormancy(self):
        default = self.directory / 'foreverdb-api-key'
        with patch.object(db, 'DATA_DIR', self.directory), patch.dict('os.environ', {}, clear=True):
            self.assertEqual(db.sync()['configured'], False)
            self.assertIsNone(db.index())
            default.write_text(' default-key\n')
            self.assertEqual(db.api_key(), 'default-key')
            explicit = self.directory / 'explicit-key'
            explicit.write_text(' file-key\n')
            with patch.dict('os.environ', {'EVERLEDGER_FOREVERDB_API_KEY_FILE': str(explicit)}):
                self.assertEqual(db.api_key(), 'file-key')
                with patch.dict('os.environ', {'EVERLEDGER_FOREVERDB_API_KEY': ' env-key '}):
                    self.assertEqual(db.api_key(), 'env-key')
        self.assertEqual(self.requests, [])

    def test_pacing_and_failures_do_not_expose_credentials(self):
        clock = [0.0]
        starts = []
        original = self.get

        def timed_request(url, headers):
            starts.append(clock[0])
            return original(url, headers)

        def sleep(seconds):
            clock[0] += seconds

        with patch.object(db.time, 'monotonic', side_effect=lambda: clock[0]), \
                patch.object(db.time, 'sleep', side_effect=sleep), \
                patch.object(db, 'request', side_effect=timed_request):
            db.sync()
        self.assertEqual(len(starts), 6)
        self.assertTrue(all(b - a >= 2.1 - 1e-9 for a, b in zip(starts, starts[1:])))
        self.tomorrow()
        with patch.object(db, 'request', side_effect=core.Failure('secret-fixture', 4,
                          {'status': 503, 'response': {'echo': 'secret-fixture'}})):
            with self.assertRaises(core.Failure) as error:
                db.sync()
        self.assertNotIn('secret-fixture', str(error.exception))
        self.assertNotIn('secret-fixture', json.dumps(error.exception.body))
        for path in self.directory.glob('*.json'):
            self.assertNotIn('secret-fixture', path.read_text())
        self.assertTrue(db.sync()['complete'])

    def test_observed_list_page_uses_total_and_offset(self):
        import everledger_foreverdb_mapping as mapping
        page = json.loads((FIXTURE.parent / 'foreverdb-list.json').read_text())
        ids, following, metadata = mapping.page(page, 0)
        self.assertEqual(len(ids), 100)
        self.assertEqual(following, 100)
        self.assertEqual(metadata['vendor_total'], 1014)
        page['data'] = [{'id': n} for n in range(1001, 1015)]
        page['meta'].update(offset=1000, next=None)
        self.assertIsNone(mapping.page(page, 1000)[1])

    def test_stack_prices_and_limited_stock_are_exact(self):
        import everledger_vendors as vendors
        sale = self.fixture['details']['8681']['data']['sells'][0]
        sale.update(price=10, stack=3, pricePerItem=3.3333333333, limited=True, limitedStockCount=None)
        db.sync()
        row = next(r for r in db.lookup(2604)['vendors'] if r['npc_id'] == 8681)
        self.assertIsNone(row['price_copper'])
        self.assertEqual((row['stack_price_copper'], row['stack']), (10, 3))
        self.assertTrue(row['limited'])
        human = '\n'.join(vendors.lines(db.lookup(2604)))
        self.assertIn('10c per 3 | limited stock', human)
        self.tomorrow()
        sale.update(price=12)
        db.sync()
        row = next(r for r in db.lookup(2604)['vendors'] if r['npc_id'] == 8681)
        self.assertEqual(row['price_copper'], 4)
        self.assertEqual(row['stack'], 3)
        self.assertEqual(db.lookup(9001)['vendors'][0]['limitedStockCount'], 2)
        self.assertIn('limited stock (2)', '\n'.join(vendors.lines(db.lookup(9001))))

    def test_502_retries_once_after_requested_delay_and_counts_request(self):
        clock = [0.0]
        starts = []
        def get(url, headers):
            starts.append(clock[0])
            if len(starts) == 1:
                raise core.Failure('upstream', 4, {'status': 502, 'response': {
                    'retryable': True, 'retry_after': 75}})
            return self.get(url, headers)
        with patch.object(db, 'request', side_effect=get), \
                patch.object(db.time, 'monotonic', side_effect=lambda: clock[0]), \
                patch.object(db.time, 'sleep', side_effect=lambda seconds: clock.__setitem__(0, clock[0]+seconds)):
            result = db.sync()
        self.assertTrue(result['complete'])
        self.assertEqual(result['requests'], 7)
        self.assertGreaterEqual(starts[1]-starts[0], 75)

    def test_502_retry_at_budget_boundary_resumes_next_day(self):
        error = core.Failure('upstream', 4, {'status': 502, 'response': {'retryable': True, 'retry_after': 60}})
        with patch.object(db, 'DAILY_REQUESTS', 1), patch.object(db, 'request', side_effect=error):
            result = db.sync()
        self.assertEqual(result['requests'], 1)
        self.assertFalse(result['complete'])
        self.assertIsNone(db.index())
        self.tomorrow()
        self.assertTrue(db.sync()['complete'])

    def test_502_retry_counts_against_remaining_window_budget(self):
        with patch.object(db, 'DAILY_REQUESTS', 1):
            self.assertEqual(db.sync()['requests'], 1)
        error = core.Failure('upstream', 4, {'status': 502, 'response': {'retryable': True}})
        with patch.object(db, 'DAILY_REQUESTS', 2):
            with patch.object(db, 'request', side_effect=error) as request:
                result = db.sync()
                self.assertEqual(result['requests'], 1)
                self.assertFalse(result['complete'])
                self.assertEqual(request.call_count, 1)
            deferred = db.sync()
            self.assertEqual(deferred['requests'], 0)
            self.assertFalse(deferred['failed'])
        checkpoint = json.loads((self.directory / 'attempt.json').read_text())
        self.assertEqual([spend['requests'] for spend in checkpoint['spends']], [1, 1])

    def test_502_retry_failure_stops_after_two_requests(self):
        error = core.Failure('upstream', 4, {'status': 502, 'response': {'retryable': True, 'retry_after': 60}})
        with patch.object(db, 'request', side_effect=error):
            with self.assertRaises(core.Failure):
                db.sync()
        attempt = json.loads((self.directory / 'attempt.json').read_text())
        self.assertEqual(attempt['requests'], 2)
        self.assertTrue(attempt['failed'])

    def test_connector_sync_uses_stub_module_and_browser_ua_without_key(self):
        import sys
        module = self.directory / 'dynamic_credentials.py'
        module.write_text("""
def add_surrogate_to_request(request, slug, allowed_hosts):
    assert slug == 'custom.foreverdb'
    assert allowed_hosts == ['foreverdb.net']
    assert request.get_header('X-api-key') is None
    request.add_header('X-Surrogate-Test', 'fixture-only')
    return request
""")
        def open_fixture(opener, req, **kwargs):
            from io import BytesIO
            from urllib.parse import urlsplit
            self.assertTrue(req.get_header('User-agent').startswith('Mozilla/5.0'))
            self.assertEqual(req.get_header('X-surrogate-test'), 'fixture-only')
            self.assertIsNone(req.get_header('X-api-key'))
            path = urlsplit(req.full_url).path
            payload = self.fixture['pages'][0] if path == '/api/v1/vendors' else self.fixture['details'][path.rsplit('/',1)[1]]
            response = BytesIO(json.dumps(payload).encode())
            response.headers = {}
            return response
        self.transport.stop()
        with patch.dict('os.environ', {'EVERLEDGER_FOREVERDB_API_KEY': '',
                'EVERLEDGER_FOREVERDB_VIA_CONNECTOR': 'custom.foreverdb',
                'EVERLEDGER_DYNAMIC_CREDENTIALS_DIR': str(self.directory)}), \
                patch.dict(sys.modules), patch.object(db.urllib.request.OpenerDirector, 'open', open_fixture):
            sys.modules.pop('dynamic_credentials', None)
            with patch.object(db, 'api_key', side_effect=AssertionError('connector must not read key')):
                result = db.sync()
                self.assertTrue(result['complete'])
                self.assertEqual(db.lookup(2488)['vendors'][0]['price_copper'], 536)
                import everledger_vendors as vendors
                status, _ = vendors.backend_status()
                self.assertEqual(status['foreverdb']['transport'], 'connector-surrogate')
        self.transport.start()
        self.assertNotIn('fixture-only', (self.directory / 'index.json').read_text())

    def test_env_transport_outgoing_user_agent(self):
        from io import BytesIO
        def open_fixture(opener, req, **kwargs):
            self.assertTrue(req.get_header('User-agent').startswith('Mozilla/5.0'))
            self.assertEqual(req.get_header('X-api-key'), 'secret-fixture')
            response = BytesIO(b'{}')
            response.headers = {}
            return response
        self.transport.stop()
        with patch.object(db.urllib.request.OpenerDirector, 'open', open_fixture):
            db.request('https://foreverdb.net/api/v1/vendors?limit=100&offset=0',
                       {'User-Agent': db.USER_AGENT, 'X-API-Key': 'secret-fixture'})
        self.transport.start()

    def test_missing_connector_module_is_clear_and_status_still_works(self):
        with patch.dict('os.environ', {'EVERLEDGER_FOREVERDB_VIA_CONNECTOR': 'custom.foreverdb'}), \
                patch.object(db.importlib, 'import_module', side_effect=ModuleNotFoundError()):
            with self.assertRaisesRegex(core.Failure, 'dynamic_credentials.*EVERLEDGER_DYNAMIC_CREDENTIALS_DIR'):
                db.sync()
            import everledger_vendors as vendors
            status, _ = vendors.backend_status()
            self.assertEqual(status['foreverdb']['transport'], 'connector-surrogate')
            self.assertIsNone(db.index())
        self.assertEqual(self.requests, [])

    def test_both_transports_refuse_redirects(self):
        import http.server
        import threading
        import types
        import sys
        hits = []
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                hits.append(self.path)
                self.send_response(302)
                self.send_header('Location', '/redirected')
                self.end_headers()
            def log_message(self, *args):
                pass
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        module = types.ModuleType('dynamic_credentials')
        module.add_surrogate_to_request = lambda req, slug, allowed_hosts: req
        self.transport.stop()
        try:
            for slug in ('', 'custom.foreverdb'):
                with self.subTest(transport=slug or 'env-key'), \
                        patch.dict('os.environ', {'EVERLEDGER_FOREVERDB_VIA_CONNECTOR': slug}), \
                        patch.dict(sys.modules, {'dynamic_credentials': module}):
                    with self.assertRaises(core.Failure) as error:
                        db.request(f'http://127.0.0.1:{server.server_port}/start', {})
                    self.assertEqual(error.exception.body['status'], 302)
            self.assertEqual(hits, ['/start', '/start'])
        finally:
            self.transport.start()
            server.shutdown()
            server.server_close()
            thread.join()

    def test_connector_exceptions_are_redacted_and_journaled(self):
        import sys
        sentinel = 'secret-fixture-surrogate'
        cases = [
            f"raise RuntimeError('{sentinel}')\n",
            f"def add_surrogate_to_request(*args, **kwargs):\n    raise RuntimeError('{sentinel}')\n",
            f"from everledger_core import Failure\ndef add_surrogate_to_request(*args, **kwargs):\n    raise Failure('{sentinel}', 4, {{'response': '{sentinel}'}})\n",
        ]
        self.transport.stop()
        try:
            with patch.dict('os.environ', {'EVERLEDGER_FOREVERDB_VIA_CONNECTOR': 'custom.foreverdb',
                    'EVERLEDGER_DYNAMIC_CREDENTIALS_DIR': str(self.directory)}), \
                    patch.object(sys, 'dont_write_bytecode', True):
                for code in cases:
                    with self.subTest(code=code), patch.dict(sys.modules):
                        sys.modules.pop('dynamic_credentials', None)
                        (self.directory / 'dynamic_credentials.py').write_text(code)
                        db.importlib.invalidate_caches()
                        (self.directory / 'attempt.json').unlink(missing_ok=True)
                        with self.assertRaises(core.Failure) as error:
                            db.sync()
                        self.assertNotIn(sentinel, str(error.exception))
                        self.assertNotIn(sentinel, json.dumps(error.exception.body))
                        attempt = json.loads((self.directory / 'attempt.json').read_text())
                        self.assertTrue(attempt['failed'])
                        for path in self.directory.glob('*.json'):
                            self.assertNotIn(sentinel, path.read_text())
        finally:
            self.transport.start()


if __name__ == '__main__':
    unittest.main()
