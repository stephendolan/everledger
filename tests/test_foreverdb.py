"""No network: synthetic API fixtures are provisional until key-day acceptance."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import everledger_core as core
import everledger_foreverdb as db

FIXTURE = Path(__file__).parent / 'fixtures/foreverdb.json'


class ForeverDB(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)
        self.fixture = json.loads(FIXTURE.read_text())
        self.requests = []
        self.key = patch.dict('os.environ', {'EVERLEDGER_FOREVERDB_API_KEY': 'secret-fixture'})
        self.key.start()
        self.addCleanup(self.key.stop)
        self.paths = patch.object(db, 'CACHE', self.directory)
        self.paths.start()
        self.addCleanup(self.paths.stop)
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
        parsed = urlsplit(url)
        if parsed.path == '/api/v1/vendors':
            page = int(parse_qs(parsed.query)['page'][0])
            data = self.fixture['pages'][page - 1]
        else:
            data = self.fixture['details'][parsed.path.rsplit('/', 1)[1]]
        return json.dumps(data).encode(), {}

    def tomorrow(self):
        path = self.directory / 'attempt.json'
        state = json.loads(path.read_text())
        state['attempted_at'] = '2020-01-01T00:00:00Z'
        state['finished_at'] = '2020-01-01T00:00:00Z'
        path.write_text(json.dumps(state))

    def test_sync_and_sale_labels_and_locations(self):
        result = db.sync()
        self.assertTrue(result['complete'])
        self.assertEqual(result['requests'], 6)
        data = db.lookup(2604)
        self.assertEqual({v['name'] for v in data['vendors']}, {
            'Poranna Snowbraid', 'Outfitter Eric', 'Burbik Gearspanner', 'Bombus Finespindle'})
        rows = {v['name']: v for v in data['vendors']}
        self.assertEqual(rows['Burbik Gearspanner']['basis'], 'baseline')
        self.assertEqual(rows['Burbik Gearspanner']['vendor_basis'], 'seen')
        self.assertEqual(rows['Bombus Finespindle']['basis'], 'seen')
        self.assertEqual(rows['Bombus Finespindle']['vendor_basis'], 'baseline')
        self.assertEqual(len(rows['Burbik Gearspanner']['locations']), 2)
        self.assertEqual(rows['Burbik Gearspanner']['coords'], [[46.4, 26.9], [48.1, 59.2]])
        self.assertEqual(rows['Outfitter Eric']['price_copper'], 50)
        self.assertIn('Data from ForeverDB.net', data['sources'][0]['detail'])
        self.assertIn('https://foreverdb.net', data['sources'][0]['detail'])

    def test_no_index_record_and_known_empty_after_removal(self):
        db.sync()
        self.assertIsNone(db.lookup(999999)['vendors'])
        self.assertEqual(db.lookup(999999)['reason'], 'item not in dataset')
        self.fixture['details']['2691']['sells'].pop()
        self.fixture['pages'][0]['vendors'][0]['updated_at'] = 'v2'
        self.tomorrow()
        result = db.sync()
        self.assertEqual(result['requests'], 3)
        self.assertEqual(db.lookup(9001)['vendors'], [])
        self.assertIsNone(db.lookup(9001)['reason'])
        self.assertEqual(len(db.lookup(2604)['vendors']), 4)

    def test_daily_refresh_and_incremental_requests(self):
        db.sync()
        self.requests.clear()
        self.assertEqual(db.sync()['requests'], 0)
        self.assertEqual(self.requests, [])
        self.tomorrow()
        result = db.sync()
        self.assertEqual(result['requests'], 2)
        self.assertEqual(len(self.requests), 2)

    def test_interrupted_sync_preserves_previous_index(self):
        db.sync()
        before = (self.directory / 'index.json').read_bytes()
        self.tomorrow()
        self.fixture['pages'][0]['vendors'][0]['updated_at'] = 'v2'
        with patch.object(db, 'request', side_effect=core.Failure('fixture failure', 4)):
            with self.assertRaises(core.Failure):
                db.sync()
        self.assertEqual((self.directory / 'index.json').read_bytes(), before)
        self.assertEqual(len(db.lookup(2604)['vendors']), 4)

    def test_budgeted_bootstrap_resumes_without_activating_partial_index(self):
        template = self.fixture['details']['2691']
        self.fixture['pages'] = [{'vendors': [{'id': n, 'updated_at': 'v1'} for n in range(1, 41)],
                                  'next_page': None, 'build': 'fixture',
                                  'extracted_at': '2026-11-13T00:00:00Z'}]
        self.fixture['details'] = {str(n): dict(template, id=n) for n in range(1, 41)}
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

    def test_mid_detail_interrupt_resumes_and_old_index_survives(self):
        db.sync()
        before = (self.directory / 'index.json').read_bytes()
        self.tomorrow()
        self.fixture['pages'][0]['vendors'][0]['updated_at'] = 'v2'
        self.fixture['pages'][0]['vendors'][1]['updated_at'] = 'v2'
        original = self.get

        def interrupt(url, headers):
            if url.endswith('/3072'):
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
        self.assertEqual(result['requests'], 1)
        self.assertEqual(len(db.lookup(2604)['vendors']), 4)

    def test_unknown_labels_and_malformed_shapes_do_not_invent_provenance(self):
        del self.fixture['details']['2691']['sells'][0]['seen']
        db.sync()
        row = next(row for row in db.lookup(2604)['vendors'] if row['npc_id'] == 2691)
        self.assertEqual(row['basis'], 'unverified')
        self.tomorrow()
        self.fixture['pages'][0]['vendors'][0].pop('updated_at')
        with self.assertRaises(core.Failure):
            db.sync()
        self.assertEqual(len(db.lookup(2604)['vendors']), 4)

    def test_abrupt_exit_cooldown_uses_the_last_request_start(self):
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

    def test_changed_extraction_discards_checkpoint_and_recovers_next_day(self):
        with patch.object(db, 'DAILY_REQUESTS', 1):
            self.assertFalse(db.sync()['complete'])
        self.tomorrow()
        self.fixture['pages'][1]['build'] = 'next-build'
        with self.assertRaises(core.Failure):
            db.sync()
        self.assertIsNone(db.index())
        self.assertFalse((self.directory / 'pending.json').exists())
        self.assertEqual(db.sync()['requests'], 0)
        self.tomorrow()
        self.fixture['pages'][0]['build'] = 'next-build'
        self.assertTrue(db.sync()['complete'])
        self.assertEqual(len(db.lookup(2604)['vendors']), 4)

    def test_failed_index_stays_ineligible_through_incomplete_retry(self):
        import everledger_vendors as vendors
        db.sync()
        attempt = json.loads((self.directory / 'attempt.json').read_text())
        attempt.update(failed=True, status=401)
        (self.directory / 'attempt.json').write_text(json.dumps(attempt))
        self.assertIsNone(vendors.active_index())
        self.tomorrow()
        template = self.fixture['details']['2691']
        self.fixture['pages'] = [{'vendors': [{'id': n, 'updated_at': 'v1'} for n in range(1, 41)],
                                  'next_page': None, 'build': 'fixture',
                                  'extracted_at': '2026-11-13T00:00:00Z'}]
        self.fixture['details'] = {str(n): dict(template, id=n) for n in range(1, 41)}
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
        self.assertEqual(db.sync()['requests'], 0)


if __name__ == '__main__':
    unittest.main()
