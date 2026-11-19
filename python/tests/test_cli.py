import datetime
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import unittest
import urllib.parse

ROOT = Path(__file__).resolve().parents[1]
FIX = ROOT.parent / 'tests/fixtures'
NOW = datetime.datetime.now(datetime.UTC).isoformat().replace('+00:00', 'Z')
MALFORMED_HTTP = {
    'bad status line': b'HTTP/1.1 abc\r\n\r\n',
    'truncated body': b'HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\n{"data": [',
    'truncated error body': b'HTTP/1.1 404 Not Found\r\nContent-Length: 500\r\n\r\n{"error":',
}

class Handler(http.server.BaseHTTPRequestHandler):
    routes = {}
    requests = []
    event_contract = None

    def do_GET(self):
        self.requests.append(self.path)
        if self.event_contract and self.path.split('?')[0] == '/events':
            status, payload = self.event_contract(urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query))
        else:
            status, payload = self.routes.get(self.path, self.routes.get(self.path.split('?')[0], (404, {})))
        if isinstance(payload, bytes):
            self.wfile.write(payload)
            self.close_connection = True
            return
        self.send_response(status)
        self.send_header('Last-Modified', 'Sat, 14 Nov 2026 20:20:39 GMT')
        self.end_headers()
        self.wfile.write((json.dumps(payload) if not isinstance(payload, str) else payload).encode())

    def log_message(self, *args):
        pass

class CLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        Path(self.tmp.name, 'catalogue.json').write_bytes((FIX / 'catalogue-fixture.json').read_bytes())
        base = 'http://127.0.0.1:' + str(self.server.server_port)
        self.env = dict(os.environ, TZ='UTC', EVERLEDGER_DATA_DIR=self.tmp.name,
                        EVERLEDGER_ROSTER_URL=base, EVERLEDGER_ROSTER_TOKEN='fixture',
                        EVERLEDGER_AHLEDGER_URL=base, EVERLEDGER_VENDOR_URL=base)
        self.env.pop('EVERLEDGER_FOREVERDB_DAILY_REQUESTS', None)
        self.env.pop('EVERLEDGER_FOREVERDB_API_KEY', None)
        self.env.pop('EVERLEDGER_FOREVERDB_API_KEY_FILE', None)
        self.env.pop('EVERLEDGER_FOREVERDB_VIA_CONNECTOR', None)
        self.env.pop('EVERLEDGER_DYNAMIC_CREDENTIALS_DIR', None)
        Handler.requests = []
        Handler.event_contract = None
        Handler.routes = {
            '/items/2604.json': (200, json.loads((FIX / '2604.json').read_text())),
            '/items/2604.pins.json': (200, json.loads((FIX / '2604.pins.json').read_text())),
            '/events': (200, {'entries': [], 'next_cursor':None, 'checkpoint':'opaque+/=', 'has_more':False,
                               'sources':[{'file':'Journalator.lua','mtime':'2020-01-01T00:00:00Z','stale':True}]}),
            '/listings': (200, {'listings':[], 'sources':[]}),
            '/characters': (200, {'characters':[], 'sources':[]}),
            '/characters/Slade': (200, {'character':{'name':'Slade Undertow'}}),
            '/characters/Slade%20Undertow': (200, {'character':{'name':'Slade Undertow'}}),
        }

    def run_cli(self, *args, code=0):
        r = subprocess.run([str(ROOT/'everledger'), *args], env=self.env, text=True, capture_output=True)
        self.assertEqual(r.returncode, code, r.stderr + r.stdout)
        return r

    def live_contract(self):
        rows = [self.event('purchase', 1, -134, id='leather', item={'id':2318,'name':'Light Leather'}),
                self.event('purchase', 1, -1960, id='cloth', item={'id':2997,'name':'Bolt of Woolen Cloth'}),
                *[self.event('sale', 1, 100, id=str(i)) for i in range(6)]]
        checkpoint_scopes = {}
        def respond(query):
            for key in ('since', 'until'):
                if key in query and not query[key][0].isdigit():
                    return 400, {'error_code':'invalid_query', 'message':'Expected bounded decimal integer'}
            limit = int(query.get('limit', ['500'])[0])
            if not 1 <= limit <= 500:
                return 400, {'error_code':'invalid_query', 'message':'Integer outside allowed bounds'}
            scope = {k:query[k] for k in ('char','item','venue','type') if k in query}
            if 'after' in query and scope != checkpoint_scopes.get(query['after'][0], {}):
                return 400, {'error_code':'checkpoint_scope_mismatch', 'message':'Checkpoint scope changed'}
            offset = int(query.get('cursor', ['page:0'])[0].split(':')[1])
            if 'after' in query:
                offset = int(query['after'][0].split(':')[1])
            page = rows[offset:offset+min(limit, 3)]
            more = offset + len(page) < len(rows)
            checkpoint = f'jnrc1:{len(rows)}' + (':'+urllib.parse.urlencode(scope) if scope else '')
            checkpoint_scopes[checkpoint] = scope
            return 200, {'entries':page, 'mode':'incremental' if 'after' in query else 'snapshot',
                         'has_more':more, 'next_cursor':f'page:{offset+len(page)}' if more else None,
                         'checkpoint':None if more else checkpoint, 'filters':{'since':int(query.get('since',['0'])[0])}, 'sources':[]}
        Handler.event_contract = staticmethod(respond)
        return rows

    def test_live_bootstrap_returns_terminal_checkpoint_without_entries(self):
        self.live_contract()
        d = json.loads(self.run_cli('events','--limit','0','--type','purchase','--json').stdout)
        self.assertEqual(d['entries'], [])
        self.assertEqual(d['after_cursor'], 'jnrc1:8')
        self.assertEqual(d['checkpoint'], 'jnrc1:8')
        self.assertIsNone(d['next_cursor'])
        self.assertFalse(d['has_more'])
        self.assertGreater(len(Handler.requests), 1)

    def test_live_time_inputs_and_outcome_commands(self):
        self.live_contract()
        self.market()
        Handler.routes['/listings'] = (200, {'listings':[
            {'item_id':100,'name':'Red Woolen Boots', 'count':1, 'price':{'recorded':{'buyout_copper':2499}}},
            {'item_id':101,'name':'Hands of Darkness', 'count':1, 'price':{'recorded':{'buyout_copper':4298}}}], 'sources':[]})
        d = json.loads(self.run_cli('events','--since','2026-11-08T00:00:00Z','--json').stdout)
        query = urllib.parse.parse_qs(urllib.parse.urlsplit(Handler.requests[0]).query)
        self.assertEqual(query['since'], ['1794096000'])
        self.run_cli('events','--json')
        for command in ('purchases','sales','expirations','net','positions','listings'):
            args = [command] + (['2604'] if command == 'net' else []) + ['--json']
            d = json.loads(self.run_cli(*args).stdout)
            if command == 'purchases':
                self.assertEqual(len(d['purchases']), 2)
                self.assertEqual(sum(e['cash_copper'] for e in d['purchases']), -2094)
            if command == 'listings':
                self.assertEqual([r['buyout_copper'] for r in d['listings']], [2499, 4298])

    def test_live_checkpoint_allows_changed_views_in_both_directions(self):
        rows = self.live_contract()
        d = json.loads(self.run_cli('events','--limit','0','--type','sale','--json').stdout)
        checkpoint = d['after_cursor']
        rows.append(self.event('purchase', 1, -50, id='new-purchase'))
        d = json.loads(self.run_cli('events','--after',checkpoint,'--type','purchase','--json').stdout)
        self.assertEqual([e['id'] for e in d['entries']], ['new-purchase'])
        rows.append(self.event('sale', 1, 60, id='new-sale'))
        d = json.loads(self.run_cli('events','--after',d['after_cursor'],'--json').stdout)
        self.assertEqual([e['id'] for e in d['entries']], ['new-sale'])
        d = json.loads(self.run_cli('events','--after',d['after_cursor'],'--char','Slade','--venue','auction','--json').stdout)
        self.assertEqual(d['entries'], [])
        self.assertEqual(d['after_cursor'], 'jnrc1:10')
        rows.extend([self.event('purchase',1,-1,id='other-char',character='Other'),
                     self.event('purchase',1,-2,id='other-venue',venue='vendor'),
                     self.event('purchase',1,-3,id='other-item',item={'id':1,'name':'Other'}),
                     self.event('purchase',1,-4,id='match')])
        d = json.loads(self.run_cli('events','--after',d['after_cursor'],'--char','Slade',
                                   '--item','2604','--venue','auction','--type','purchase','--json').stdout)
        matched = list(d['entries'])
        while d['has_more']:
            d = json.loads(self.run_cli('events','--cursor',d['next_cursor'],'--json').stdout)
            matched.extend(d['entries'])
        self.assertEqual([e['id'] for e in matched], ['match'])
        self.assertEqual(d['after_cursor'], 'jnrc1:14')

    def test_live_view_survives_snapshot_pagination(self):
        self.live_contract()
        d = json.loads(self.run_cli('events','--type','sale','--limit','2','--json').stdout)
        rows = list(d['entries'])
        seen = set()
        while d['has_more']:
            self.assertIsNone(d['after_cursor'])
            cursor = d['next_cursor']
            self.assertNotIn(cursor, seen)
            seen.add(cursor)
            d = json.loads(self.run_cli('events','--cursor',cursor,'--limit','2','--json').stdout)
            rows.extend(d['entries'])
        self.assertEqual(len(rows), 6)
        self.assertTrue(all(e['type'] == 'sale' for e in rows))
        self.assertEqual(d['after_cursor'], 'jnrc1:8')

    def test_expired_view_page_keeps_until_on_resync(self):
        sale = self.event('sale', 1, 90)
        Handler.routes['/events'] = (200, {'entries':[sale], 'has_more':True, 'next_cursor':'bad-page'})
        d = json.loads(self.run_cli('events','--type','sale','--until','2099-01-01','--json').stdout)
        Handler.routes['/events?cursor=bad-page'] = (409, {'error_code':'cursor_invalid','resync_since':1794614400})
        Handler.routes['/events?until=4070908800&since=1794614400'] = (200, {
            'entries':[sale,self.event('purchase',1,-100)], 'has_more':False, 'checkpoint':'recovered'})
        r = self.run_cli('events','--cursor',d['next_cursor'],'--json')
        self.assertEqual([e['type'] for e in json.loads(r.stdout)['entries']], ['sale'])
        self.assertEqual(Handler.requests[-1], '/events?until=4070908800&since=1794614400')

    def test_cursor_bootstrap_resync_keeps_view_and_until(self):
        sale = self.event('sale', 1, 90)
        Handler.routes['/events'] = (200, {'entries':[sale], 'has_more':True, 'next_cursor':'bad-page'})
        d = json.loads(self.run_cli('events','--type','sale','--until','2099-01-01','--json').stdout)
        Handler.routes['/events?cursor=bad-page&limit=500'] = (409, {'error_code':'cursor_invalid','resync_since':1794614400})
        Handler.routes['/events?limit=500&until=4070908800&since=1794614400'] = (200, {
            'entries':[sale], 'has_more':False, 'checkpoint':'recovered'})
        r = self.run_cli('events','--cursor',d['next_cursor'],'--limit','0','--json')
        self.assertEqual(json.loads(r.stdout)['entries'], [])
        self.assertEqual(json.loads(r.stdout)['after_cursor'], 'recovered')
        self.assertIn('until=4070908800', Handler.requests[-1])

    def test_live_invalid_type_is_a_usage_error(self):
        self.live_contract()
        self.run_cli('events','--type','invalid','--json',code=2)
        self.assertFalse(Handler.requests)

    def test_resync_normalizes_integer_and_decimal_epoch_hints(self):
        for hint in (1794614400, '1794614400', 0):
            Handler.routes['/events?limit=500&after=bad'] = (409, {'error_code':'cursor_invalid','resync_since':hint})
            r = self.run_cli('events','--after','bad','--json')
            self.assertIn('since='+str(hint), Handler.requests[-1])
            self.assertEqual(r.stderr.count('resynchronizing'), 1)

    def test_bootstrap_refuses_repeated_page_cursor(self):
        Handler.routes['/events'] = (200, {'entries':[], 'has_more':True, 'next_cursor':'stuck'})
        r = self.run_cli('events','--limit','0','--json',code=1)
        self.assertIn('pagination did not advance', r.stderr)

    def output_fixture(self):
        self.market()
        journal = {'file':'Journalator.lua', 'saved_at':'2020-01-01T00:00:00Z', 'stale':True}
        scans = [{'available':True, 'pricing':{'available':True, 'source':{
            'file':'DataStore_Auctions.lua', 'scanned_at':NOW, 'stale':False}}} for _ in range(10)]
        Handler.routes['/listings'] = (200, {'listings':[
            {'item_id':2604, 'name':'Red Dye', 'character':'Slade', 'count':2},
            {'item_id':100, 'name':'Red Woolen Boots', 'character':'Slade', 'count':1}],
            'scans':scans, 'matched_count':99,'returned_count':99,
            'sources':[s['pricing']['source'] for s in scans]})
        Handler.routes['/events'] = (200, {'entries':[
            self.event(t, None, None, character=None, recorded_at=None)
            for t in ('purchase','sale','expiration')], 'sources':[journal],
            'next_cursor':None, 'has_more':False, 'checkpoint':None})
        return [('listings',), ('sales',), ('purchases',), ('expirations',), ('events',),
                ('net','2604'), ('positions',), ('vendors','2604'), ('price','2604'),
                ('margins','tailoring')]

    def test_human_unknowns_preserve_json_nulls(self):
        commands = self.output_fixture()
        quote = Handler.routes['/v1/prices/forever.pvp.alliance.us/2604'][1]
        quote['latest'].update(quantity=None, auctions=None)
        quote['sales'].update(estimatedPerDay=None, coveredHours=None)
        Handler.routes['/v1/pricetable/forever.pvp.alliance.us'] = (200,
            Handler.routes['/v1/pricetable/forever.pvp.alliance.us'][1].replace(':::::3808', ':::::').replace(':::::10', ':::::'))
        for args in commands:
            with self.subTest(command=args):
                human = self.run_cli(*args).stdout
                self.assertNotRegex(human, r'\b(?:None|null|unknownx|unknownh)\b|\|\s*\|')
                if args[0] in ('sales','purchases','expirations','events'):
                    self.assertIn('quantity unknown', human)
                if args[0] == 'price':
                    self.assertIn('coverage unknown', human)
                    self.assertIn('sales/day unknown', human)
        self.assertIn('undercut unknown', self.run_cli('listings').stdout)
        data = json.loads(self.run_cli('listings','--json').stdout)
        self.assertIsNone(data['listings'][0]['undercut'])
        self.assertIsNone(data['listings'][0]['expires_at'])
        self.assertEqual((data['matched_count'],data['returned_count']), (99,99))

    def test_human_has_one_trailing_source_summary(self):
        for args in self.output_fixture():
            with self.subTest(command=args):
                lines = self.run_cli(*args).stdout.splitlines()
                self.assertEqual(sum(line.startswith('source:') for line in lines), 1)
                self.assertTrue(lines[-1].startswith('source:'))
                self.assertIn('age', lines[-1])
        data = json.loads(self.run_cli('listings','--json').stdout)
        self.assertEqual(len(data['scans']), 10)
        self.assertEqual(len(data['sources']), 2)
        self.assertEqual(len(data['scans']), 10)

    def test_json_counts_match_filtered_arrays_and_aggregate_pages(self):
        rows = [self.event(t, 1, -10, id=str(i)) for i, t in enumerate(
            ['purchase'] * 4 + ['sale'] * 2 + ['expiration'] * 2)]
        Handler.routes['/events'] = (200, {'entries':rows[:5], 'has_more':True,
            'next_cursor':'second', 'matched_count':8, 'returned_count':5, 'sources':[]})
        Handler.routes['/events?limit=500&cursor=second'] = (200, {
            'entries':rows[5:], 'has_more':False, 'checkpoint':'terminal',
            'matched_count':8, 'returned_count':3, 'sources':[]})
        for command, expected in [('purchases',4), ('sales',2), ('expirations',2)]:
            with self.subTest(command=command):
                if command == 'expirations':
                    self.market()
                data = json.loads(self.run_cli(command,'--json').stdout)
                self.assertEqual(len(data[command]), expected)
                self.assertEqual((data['matched_count'],data['returned_count']), (8,3))
        data = json.loads(self.run_cli('events','--type','sale','--json').stdout)
        self.assertEqual((data['matched_count'],data['returned_count']), (8,5))
        data = json.loads(self.run_cli('events','--limit','0','--json').stdout)
        self.assertEqual((data['matched_count'],data['returned_count']), (8,3))

    def test_stale_notice_and_exit_code_follow_the_same_freshness_decision(self):
        commands = self.output_fixture()
        self.market(stale=True)
        Handler.routes['/listings'][1]['scans'][0]['pricing']['source']['stale'] = True
        for args in commands:
            with self.subTest(command=args):
                code = 3 if args[0] in ('price','margins','listings','positions','expirations','net') else 0
                result = self.run_cli(*args, code=code)
                lines = result.stdout.splitlines()
                notices = [i for i, line in enumerate(lines) if line.startswith('stale:')]
                if code == 0:
                    self.assertEqual(notices, [])
                else:
                    self.assertEqual(notices, [len(lines)-2])
                self.run_cli(*args, '--json', code=code)
                self.assertTrue(lines[-1].startswith('source:'))
                self.assertNotIn('stale', lines[-1])
                self.assertNotIn('stale', result.stderr)
                if args[0] in ('sales','purchases','expirations','events','net','positions'):
                    self.assertIn('Journalator.lua', lines[-1])
                    self.assertIn('2020-01-01', lines[-1])

    def test_roster_derived_results_share_stale_notice_and_exit_code(self):
        self.market()
        ch = {'name':'Slade', 'professions':[{'name':'Tailoring', 'skill':5,
              'max_skill':75, 'skill_available':True,
              'recipes':{'available':True, 'known':[{'spell_id':1}]}}]}
        stale_source = {'file':'Journalator-looking-name.lua', 'mtime':NOW, 'stale':True}
        Handler.routes['/characters'] = (200, {'characters':[ch], 'sources':[stale_source]})
        Handler.routes['/characters/Slade'] = (200, {'character':ch, 'sources':[stale_source]})
        for args in [('recipe', 'Red Shirt'), ('char', 'Slade'),
                     ('margins', 'tailoring', '--char', 'Slade')]:
            with self.subTest(command=args):
                result = self.run_cli(*args, code=3)
                self.assertEqual(result.stdout.count('stale: data exceeds'), 1)
                self.run_cli(*args, '--json', code=3)

    def test_status_probes_one_event_page(self):
        self.live_contract()
        self.market()
        Handler.routes['/'] = (200, '')
        self.run_cli('status', '--json', code=1)
        self.assertEqual([p for p in Handler.requests if p.startswith('/events')],
                         ['/events?limit=1'])

    def test_status_without_vendor_cache_has_no_nulls(self):
        self.market()
        Handler.routes['/'] = (200, '')
        Handler.routes['/characters'][1]['state'] = None
        result = self.run_cli('status', code=1)
        self.assertNotRegex(result.stdout, r'\b(?:None|null)\b|\|\s*\|')
        self.assertIn('vendors data: no cache yet', result.stdout)
        data = json.loads(self.run_cli('status', '--json', code=1).stdout)
        vendor = next(row for row in data['status'] if row['source'] == 'vendors')
        self.assertIsNone(vendor['published_at'])
        self.assertEqual(vendor['cache_items'], 0)

    def test_status_pending_uses_words_and_keeps_json_booleans(self):
        self.market()
        Handler.routes['/'] = (200, '')
        self.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
        pending_path = Path(self.tmp.name, 'vendors/foreverdb.net/pending.json')
        pending_path.parent.mkdir(parents=True)
        for pending, word in ((False, 'no'), (True, 'yes')):
            with self.subTest(pending=pending):
                if pending:
                    pending_path.write_text('checkpoint contents need not be parsed')
                human = self.run_cli('status', code=1).stdout
                self.assertIn(f'pending {word}', human)
                self.assertIn('last sync never', human)
                self.assertNotRegex(human, r'\b(?:True|False|None|null)\b|\?Z')
                data = json.loads(self.run_cli('status', '--json', code=1).stdout)
                vendor = next(row for row in data['status'] if row['source'] == 'vendors')
                self.assertIs(vendor['foreverdb']['pending'], pending)
                self.assertIsNone(vendor['foreverdb']['last_sync_at'])

    def test_ninja_status_without_published_times_is_null(self):
        self.run_cli('vendors', '2604')
        path = Path(self.tmp.name, 'vendors/wowforever.ninja/2604.json')
        data = json.loads(path.read_text())
        data['published_at'] = None
        path.write_text(json.dumps(data))
        self.market()
        Handler.routes['/'] = (200, '')
        data = json.loads(self.run_cli('status', '--json', code=1).stdout)
        self.assertIsNone(next(r for r in data['status'] if r['source'] == 'vendors')['published_at'])

    def test_character_timestamps_use_unknown_and_preserve_known_values(self):
        for generated, updated in ((None, None), (NOW, None), (None, NOW),
                                   (NOW, NOW), ('2026-11-15T09:10:11-05:00', '2026-11-15T09:10:11-05:00')):
            with self.subTest(generated=generated, updated=updated):
                Handler.routes['/characters/Slade'] = (200, {'generated_at': generated,
                    'character': {'name': 'Slade', 'professions_updated_at': updated}})
                human = self.run_cli('char', 'Slade').stdout
                self.assertIn(f"roster snapshot generated {generated or 'unknown'}; professions updated {updated or 'unknown'}", human)
                self.assertNotRegex(human, r'\?Z|unknownZ|\b(?:None|null)\b')
                data = json.loads(self.run_cli('char', 'Slade', '--json').stdout)
                self.assertEqual(data['generated_at'], generated)
                self.assertEqual(data['character']['professions_updated_at'], updated)

    def test_event_pagination_status_uses_words_and_keeps_json_booleans(self):
        for more, word in ((False, 'no'), (True, 'yes')):
            with self.subTest(has_more=more):
                Handler.routes['/events'] = (200, {'entries': [], 'has_more': more,
                    'next_cursor': 'second' if more else None, 'checkpoint': None if more else 'terminal'})
                human = self.run_cli('events').stdout
                self.assertIn(f'has_more: {word}', human)
                self.assertNotRegex(human, r'\b(?:True|False|None|null)\b')
                data = json.loads(self.run_cli('events', '--json').stdout)
                self.assertIs(data['has_more'], more)
                self.assertIsNone(data['after_cursor'] if more else data['next_cursor'])

    def test_status_groups_roster_files_only_in_human_footer(self):
        self.market()
        Handler.routes['/'] = (200, '')
        files = [{'file':f'/saved/File{i}.lua', 'mtime':NOW, 'file_age_seconds':i * 10}
                 for i in range(19)]
        for route in ('/characters', '/events', '/listings'):
            Handler.routes[route][1]['sources'] = files
        human = self.run_cli('status', code=1).stdout
        footer = human.splitlines()[-1]
        self.assertEqual(human.count('source:'), 1)
        self.assertIn('roster service (19 files; oldest age 180s, newest 0s)', footer)
        self.assertNotIn('.lua', footer)
        for distinct in ('wowforever.ninja profession index pages', 'AHledger', 'wowforever.ninja'):
            self.assertIn(distinct, footer)
        data = json.loads(self.run_cli('status', '--json', code=1).stdout)
        self.assertEqual([s for s in data['sources'] if 'file' in s], files * 3)

    def test_aggregate_pages_union_sources_and_keep_age_range(self):
        self.market()
        old = {'file':'Journalator.lua', 'mtime':'2026-11-14T20:00:00Z', 'file_age_seconds':900}
        recent = dict(old, mtime='2026-11-14T20:14:00Z', file_age_seconds=60)
        shared = {'file':'Other.lua', 'mtime':NOW, 'file_age_seconds':300}
        rows = [self.event(t, 1, 90, id=t, proceeds_copper=90)
                for t in ('sale', 'purchase', 'expiration')]
        Handler.routes['/events'] = (200, {'entries':rows, 'has_more':True,
            'next_cursor':'second', 'sources':[old, shared]})
        Handler.routes['/events?limit=500&cursor=second'] = (200, {
            'entries':[dict(row, id=row['id']+'2') for row in rows],
            'has_more':False, 'checkpoint':'terminal', 'sources':[recent, shared]})
        for args in [('events','--limit','0'), ('sales',), ('purchases',),
                     ('expirations',), ('net','2604'), ('positions',)]:
            with self.subTest(command=args):
                data = json.loads(self.run_cli(*args, '--json').stdout)
                self.assertEqual([s for s in data['sources'] if 'file' in s], [old, shared, recent])
                footer = self.run_cli(*args).stdout.splitlines()[-1]
                self.assertIn('age 60-900s', footer)
                self.assertIn('2026-11-14T20:00:00Z..2026-11-14T20:14:00Z', footer)
                self.assertEqual(footer.count('Other.lua'), 1)
                if args[0] in ('sales', 'purchases', 'expirations'):
                    self.assertEqual(len(data[args[0]]), 2)

    def test_character_and_catalogue_unknowns_do_not_leak_nulls(self):
        ch = {'name':'Slade', 'level':None, 'hearth':None, 'professions':[
            {'name':'Tailoring', 'skill':5, 'max_skill':None, 'skill_available':True},
            {'name':'Mining', 'skill_available':False, 'reason':None}]}
        Handler.routes['/characters'] = (200, {'characters':[ch], 'sources':[]})
        Handler.routes['/characters/Slade'] = (200, {'character':ch, 'sources':[]})
        path = Path(self.tmp.name, 'catalogue.json')
        cat = json.loads(path.read_text())
        recipe = cat['professions']['tailoring']['recipes'][0]
        recipe['bands'].update(yellow=None, green=None, grey=None)
        recipe['source_type'] = None
        cat['professions']['leatherworking'] = {'stated_count':1, 'recipes':[{
            **recipe, 'name':'Cloudy Gustwoven Boots', 'bands':{**recipe['bands'], 'learn':55}}]}
        path.write_text(json.dumps(cat))
        for args in [('char',), ('char','Slade'), ('char','Slade','tailoring'),
                     ('bridge','tailoring','1','20'), ('recipe','Red Shirt')]:
            with self.subTest(command=args):
                result = self.run_cli(*args, code=0)
                self.assertNotRegex(result.stdout, r'\b(?:None|null)\b')
        data = json.loads(self.run_cli('char','--json').stdout)
        self.assertIsNone(data['characters'][0]['level'])

    def test_invalid_foreverdb_daily_requests_exits_with_usage_error(self):
        for value in ('', 'invalid', '1.5', '0', '-1'):
            with self.subTest(value=value):
                self.env['EVERLEDGER_FOREVERDB_DAILY_REQUESTS'] = value
                result = self.run_cli('refresh', 'catalogue', '--json', code=2)
                self.assertIn('EVERLEDGER_FOREVERDB_DAILY_REQUESTS', result.stderr)
                self.assertIn('positive integer', result.stderr)
                self.assertNotIn('Traceback', result.stderr)
                self.assertEqual(result.stdout, '')
                self.assertEqual(Handler.requests, [])

    def test_readme_command_reference_matches_help(self):
        readme = (ROOT / 'README.md').read_text()
        blocks = [block.split('```', 1)[0].split()
                  for block in readme.split('```text\n')[1:]]
        for command in ('', 'price','recipe','bridge','margins','vendors','char','listings',
                        'positions','sales','purchases','expirations','net','events','status',
                        'refresh'):
            with self.subTest(command=command):
                args = [command] if command else []
                help_text = self.run_cli(*args, '--help').stdout
                self.assertIn(help_text.split(), blocks)

    def test_red_dye_ground_truth(self):
        d = json.loads(self.run_cli('vendors','Red Dye','--json').stdout)
        self.assertEqual(len(d['vendors']),89)
        self.assertEqual(sum(len(v['coords']) for v in d['vendors']),91)
        felika = next(v for v in d['vendors'] if v['name']=='Felika')
        self.assertEqual(len(felika['coords']),3)
        d = json.loads(self.run_cli('vendors','2604','--in','IF','--json').stdout)
        self.assertEqual({v['name']:(v['title'],v['coords'][0],v['price_copper']) for v in d['vendors']}, {
            'Poranna Snowbraid':('Tailoring Supplies',[42.9,28.3],50),
            'Outfitter Eric':('Speciality Tailoring Supplies',[43.4,29.3],50),
            'Burbik Gearspanner':('Trade Supplier',[46.4,26.9],50),
            'Bombus Finespindle':('Leatherworking Supplies',[39.6,34.5],50)})

    def test_three_vendor_absences(self):
        for iid, item, pins, vendors, reason in [(900,404,404,None,'item not in dataset'),
                (6532,200,404,None,'no pins published'),(901,200,200,[],None)]:
            Handler.routes[f'/items/{iid}.json']=(item, {'id':iid,'name':'Example'})
            Handler.routes[f'/items/{iid}.pins.json']=(pins, {'item':iid,'zones':{}})
            d=json.loads(self.run_cli('vendors',str(iid),'--json').stdout)
            self.assertEqual(d['vendors'],vendors)
            self.assertEqual(d.get('reason'),reason)
        self.assertIn('vendor coverage unknown',self.run_cli('vendors','6532').stdout)

    def test_cursor_pass_through_and_usage(self):
        d=json.loads(self.run_cli('events','--limit','0','--json').stdout)
        self.assertEqual(d['after_cursor'],'opaque+/=')
        self.assertIs(d['has_more'],False)
        self.run_cli('events','--after','opaque+/=','--char','Slade','--item','Red Dye','--venue','vendor','--type','sale','--json')
        self.assertIn('after=opaque%2B%2F%3D',Handler.requests[-1])
        self.assertNotIn('char=',Handler.requests[-1])
        self.assertNotIn('type=',Handler.requests[-1])
        for args in [('events','--after',''),('events','--after','x','--until','7d'),('sales','--since','bad'),('price',),('margins','tailoring','12')]:
            self.run_cli(*args, code=2)
        self.assertIn('events --limit 0 --json',self.run_cli('events','--after','',code=2).stderr)

    def test_outcomes_are_not_stale_errors(self):
        r=self.run_cli('sales','--since','7d','--until','2026-11-14','--json')
        self.assertEqual(json.loads(r.stdout)['sales'],[])
        self.assertNotIn('venue=',Handler.requests[-1])
        self.assertIn('since=',Handler.requests[-1])
        self.assertIn('Journalator',self.run_cli('sales').stdout)

    def market(self, stale=False):
        when = '2020-01-01T00:00:00Z' if stale else NOW
        stamp = int(datetime.datetime.fromisoformat(when.replace('Z','+00:00')).timestamp())
        Handler.routes['/v1/pricetable/forever.pvp.alliance.us'] = (200,
            f'market|version|{stamp}|2\n2604:88:87:709:::::3808\n100:200:190:2:::::10')
        Handler.routes['/v1/prices/forever.pvp.alliance.us/2604'] = (200, {
            'item': {'id':2604,'name':'Red Dye'}, 'latest':{'observedAt':when,
            'median':88,'minBuyout':87,'median7d':92,'quantity':709,'auctions':13},
            'sales':{'estimatedPerDay':380.8,'coveredHours':79.7}})

    def event(self, typ, count, cash, **extra):
        return {'id':typ,'type':typ,'character':'Slade Undertow','item':{'id':2604,'name':'Red Dye'},
                'count':count,'cash_copper':cash,'venue':'auction','recorded_at':NOW,**extra}

    def test_selectors_are_normalized_and_encoded(self):
        self.run_cli('events','--char','Slade Undertow','--since','24h','--until','2099-01-01','--json')
        query = urllib.parse.parse_qs(urllib.parse.urlsplit(Handler.requests[-1]).query)
        self.assertNotIn('char', query)
        self.assertTrue(query['since'][0].isdigit())
        self.assertEqual(query['until'],['4070908800'])
        for bad in [('events','--limit','-1'),('sales','--since','2026-11-15','--until','2026-11-14'),
                    ('purchases','--venue','both')]:
            self.run_cli(*bad,code=2)

    def test_sales_proceeds_and_linkless_basis(self):
        sale=self.event('sale',4,38400,value_copper=40000,proceeds_copper=38000,
                        item={'id':None,'name':'rEd dYe'})
        Handler.routes['/events']=(200,{'entries':[sale],'sources':[{'file':'Journalator.lua','mtime':'2020-01-01T00:00:00Z','stale':True}]})
        d=json.loads(self.run_cli('sales','--item','2604','--json').stdout)
        self.assertEqual(d['proceeds_copper'],38000)
        self.assertEqual(d['sales'][0]['match_basis'],'name')
        human=self.run_cli('sales','--item','Red Dye').stdout
        self.assertIn('matched by name',human)
        self.assertIn('total proceeds 3g 80s',human)
        self.assertTrue(human.strip().splitlines()[-1].startswith('source:'))

    def test_snapshot_paging_finishes_before_totals(self):
        sale=self.event('sale',1,90,proceeds_copper=90)
        Handler.routes['/events']=(200,{'entries':[sale],'has_more':True,'next_cursor':'page2','sources':[]})
        Handler.routes['/events?limit=500&cursor=page2']=(200,{'entries':[dict(sale,id='second')],'has_more':False,'next_cursor':None,'checkpoint':'terminal','sources':[]})
        d=json.loads(self.run_cli('sales','--json').stdout)
        self.assertEqual(len(d['sales']),2)
        self.assertEqual(d['proceeds_copper'],180)
        self.assertEqual(d['checkpoint'],'terminal')
        self.assertEqual(Handler.requests[-1],'/events?limit=500&cursor=page2')

    def test_cursor_invalid_resyncs_once(self):
        Handler.routes['/events?char=None']=(200,{})
        Handler.routes['/events?limit=500&after=bad']=(409,{'error_code':'cursor_invalid','resync_since':'2026-11-14T00:00:00Z'})
        r=self.run_cli('events','--after','bad','--json')
        self.assertEqual(r.stderr.count('resynchronizing'),1)
        self.assertIn('since=1794614400',Handler.requests[-1])
        self.assertEqual(json.loads(r.stdout)['after_cursor'],'opaque+/=')
        self.run_cli('events','--cursor','page','--since','7d',code=2)

    def test_price_margins_and_required_source_exit_codes(self):
        self.market()
        d=json.loads(self.run_cli('price','2604','--json').stdout)
        self.assertEqual(d['latest']['median_copper'],88)
        self.assertEqual(d['vendors'][0]['price_copper'],50)
        self.assertIn('vendor: listed price 50c',self.run_cli('price','Red Dye').stdout)
        Handler.routes['/v1/prices/forever.pvp.alliance.us/2604'][1]['latest']['quantity'] = 709.0
        self.assertIn('listed 709 in', self.run_cli('price','2604').stdout)
        self.assertIs(type(json.loads(self.run_cli('price','2604','--json').stdout)['latest']['quantity']), int)
        Handler.routes['/events'][1]['entries'] = [self.event('expiration', 1, 0)]
        self.assertIn('listed 709 |', self.run_cli('expirations').stdout)
        self.assertIs(type(json.loads(self.run_cli('expirations','--json').stdout)['expirations'][0]['market']['quantity']), int)
        d=json.loads(self.run_cli('margins','tailoring','--level','5','--json').stdout)
        self.assertEqual(d['margins'][0]['cost_copper'],88)
        self.assertEqual(d['margins'][0]['margin_copper'],102)
        self.assertEqual(d['margins'][0]['reagents'][0]['basis'],'AHledger')
        self.market(stale=True)
        self.run_cli('price','2604','--json',code=3)
        self.run_cli('margins','--json',code=3)
        Handler.routes['/events']=(503,{'error_code':'unreadable_snapshot'})
        self.assertEqual(self.run_cli('sales','--json',code=4).stdout,'')
        self.run_cli('vendors','not in catalogue',code=1)

    def test_malformed_http_reads_are_source_unreachable(self):
        routes = (
            ('/characters', ('char',)),
            ('/v1/prices/forever.pvp.alliance.us/2604', ('price', '2604')),
            ('/items/2604.json', ('vendors', '2604')),
            ('/items/2604.pins.json', ('vendors', '2604')),
        )
        for name, response in MALFORMED_HTTP.items():
            for route, command in routes:
                original = Handler.routes.get(route)
                Handler.routes[route] = (200, response)
                try:
                    for as_json in (False, True):
                        with self.subTest(response=name, route=route, json=as_json):
                            result = self.run_cli(*command, *(['--json'] if as_json else []), code=4)
                            self.assertEqual(result.stdout, '')
                            url = self.env['EVERLEDGER_VENDOR_URL'] + route
                            self.assertTrue(result.stderr.startswith(url + ': source unreachable ('), result.stderr)
                            self.assertNotIn('Traceback', result.stderr)
                finally:
                    if original is None:
                        Handler.routes.pop(route)
                    else:
                        Handler.routes[route] = original

    def test_malformed_http_sync_finalizes_attempt_and_spend(self):
        self.refresh_routes()
        self.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
        fixture = json.loads((FIX / 'foreverdb.json').read_text())
        script = """
import os, runpy
from unittest.mock import patch
import everledger_foreverdb as db
with patch.object(db, 'BASE', os.environ['EVERLEDGER_VENDOR_URL']), patch.object(db, 'INTERVAL', 0):
    runpy.run_path('everledger', run_name='__main__')
"""
        cache = Path(self.tmp.name, 'vendors/foreverdb.net')
        for name, response in MALFORMED_HTTP.items():
            for midflight in (False, True):
                for as_json in (False, True):
                    with self.subTest(response=name, midflight=midflight, json=as_json):
                        for path in cache.glob('*.json'):
                            path.unlink()
                        count = 2 if midflight else 1
                        self.env['EVERLEDGER_FOREVERDB_DAILY_REQUESTS'] = str(count)
                        Handler.routes['/api/v1/vendors'] = (200, fixture['pages'][0] if midflight else response)
                        Handler.routes['/api/v1/vendors/8681'] = (200, response)
                        command = ['python3', '-c', script, 'refresh', 'catalogue', *(['--json'] if as_json else [])]
                        result = subprocess.run(command, cwd=ROOT, env=self.env, text=True, capture_output=True)
                        self.assertEqual(result.returncode, 4, result.stderr + result.stdout)
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
                        Handler.requests.clear()
                        deferred = subprocess.run(['python3', '-c', script, 'refresh', 'catalogue', '--json'],
                                                  cwd=ROOT, env=self.env, text=True, capture_output=True)
                        self.assertEqual(deferred.returncode, 4, deferred.stderr + deferred.stdout)
                        sync = json.loads(deferred.stdout)['vendor_sync']
                        self.assertEqual((sync['requests'], sync['total_requests'], sync['deferred']),
                                         (0, count, 'daily sync limit'))
                        self.assertFalse(any(p.startswith('/api/v1/') for p in Handler.requests))

    def test_malformed_http_refresh_serves_cached_vendors_as_stale(self):
        self.refresh_routes()
        self.run_cli('vendors', '2604', '--json')
        cache = Path(self.tmp.name, 'vendors/wowforever.ninja/2604.json')
        before = cache.read_bytes()
        for name, response in MALFORMED_HTTP.items():
            for route in ('/items/2604.json', '/items/2604.pins.json'):
                original = Handler.routes[route]
                Handler.routes[route] = (200, response)
                try:
                    with self.subTest(response=name, route=route):
                        result = self.run_cli('refresh', 'catalogue', '--json', code=3)
                        self.assertEqual(result.stderr, '')
                        data = json.loads(result.stdout)
                        self.assertTrue(any(source.get('stale') for source in data['sources']))
                        self.assertEqual(cache.read_bytes(), before)
                finally:
                    Handler.routes[route] = original

    def test_vendor_status_raw_and_unknown_price(self):
        payload={'item':22,'zones':{'test':{'zone':{'name':'Test'},'vendors':{
            '1':{'n':'Changed','s':'Supply · nonsense','f':'u'},
            '2':{'n':'Unknown','s':'Supply · 1g 2s 3c','f':'r'}},
            'pins':{'vendor':[[1,2,1],[3,4,2]]},'codes':{'vendor':['c','r']}}}}
        Handler.routes['/items/22.json']=(200,{'id':22,'name':'Fixture'})
        Handler.routes['/items/22.pins.json']=(200,payload)
        d=json.loads(self.run_cli('vendors','22','--json').stdout)
        first,second=d['vendors']
        self.assertEqual((first['status_raw'],first['status'],first['price_copper']),('c','changed',None))
        self.assertEqual((second['basis'],second['price_copper']),('unverified',10203))
        self.assertEqual(first['raw'],payload['zones']['test']['vendors']['1'])
        r=self.run_cli('vendors','2604','--in','Nowhere')
        self.assertIn('known zones:',r.stdout)
        self.assertIn('Ironforge',r.stdout)

    def test_cache_failure_window(self):
        self.run_cli('vendors','2604','--json')
        path=Path(self.tmp.name,'vendors/wowforever.ninja/2604.json')
        cached=json.loads(path.read_text())
        self.assertIn('raw_pins',cached)
        cached['fetched_at']=(datetime.datetime.now(datetime.UTC)-datetime.timedelta(days=2)).isoformat()
        path.write_text(json.dumps(cached))
        self.run_cli('vendors','2604','--json',code=3)
        # Test the same public refresh operation without downloading twelve profession pages.
        script = "import everledger_vendors as v; from everledger_core import Failure; import sys;\ntry:\n used,_=v.refresh_cached(); d={'stale':any(s.get('stale') for s in used)}; print(d['stale']); sys.exit(3 if d['stale'] else 0)\nexcept Failure as e: sys.exit(e.code)"
        Handler.routes['/items/2604.json']=(503,{})
        r=subprocess.run(['python3','-c',script],cwd=ROOT,env=self.env,capture_output=True,text=True)
        self.assertEqual(r.returncode,3,r.stderr)
        cached['fetched_at']=(datetime.datetime.now(datetime.UTC)-datetime.timedelta(days=8)).isoformat()
        path.write_text(json.dumps(cached))
        r=subprocess.run(['python3','-c',script],cwd=ROOT,env=self.env,capture_output=True,text=True)
        self.assertEqual(r.returncode,4,r.stderr)
        self.assertEqual(r.stdout,'')

    def test_recipe_bridge_char_and_margins_regressions(self):
        d=json.loads(self.run_cli('recipe','Red Shirt','--skill','15','--json').stdout)
        self.assertEqual(d['recipes'][0]['bands']['learn'],1)
        self.assertIn('yellow',self.run_cli('recipe','Red Shirt','--skill','15').stdout)
        self.assertEqual(json.loads(self.run_cli('bridge','tailoring','1','20','--json').stdout)['segments'][0]['recipes'][0]['name'],'Red Shirt')
        ch={'professions_updated_at':None,'name':'Slade Undertow','short_name':'Slade','level':20,'hearth':None,'professions':[
            {'name':'Tailoring','skill':5,'max_skill':75,'skill_available':True,
             'recipes':{'available':True,'known':[{'spell_id':1}]}}]}
        Handler.routes['/characters/Slade']=(200,{'character':ch,'sources':[]})
        Handler.routes['/characters']=(200,{'characters':[ch],'sources':[]})
        self.assertEqual(json.loads(self.run_cli('char','--json').stdout)['characters'][0]['name'],'Slade Undertow')
        self.assertIn('Tailoring: 5/75',self.run_cli('char','Slade').stdout)
        self.assertIn('Red Shirt',self.run_cli('char','Slade','tailoring').stdout)
        self.market()
        r=self.run_cli('margins','tailoring','--char','Slade','--json')
        self.assertEqual(len(json.loads(r.stdout)['margins']),1)

    def test_net_signed_counts_and_unknown_profit(self):
        rows=[self.event('posting',6,-600),self.event('sale',4,38400,proceeds_copper=38000),self.event('expiration',2,0)]
        Handler.routes['/events']=(200,{'entries':rows,'available':True,'sources':[]})
        d=json.loads(self.run_cli('net','Red Dye','--json').stdout)
        self.assertEqual((d['unaccounted'],d['cash_copper'],d['proceeds_copper']),(0,37800,38000))
        self.assertIsNone(d['realized_profit_copper'])
        self.assertIsNone(d['deposits_lost_copper'])
        self.assertNotIn('deposits lost', self.run_cli('net','Red Dye').stdout)
        Handler.routes['/events']=(200,{'entries':rows[1:],'sources':[]})
        r=self.run_cli('net','Red Dye')
        self.assertIn('posted unknown',r.stdout)
        self.assertNotIn('deposits lost',r.stdout)
        rows[0]['count']=2
        Handler.routes['/events']=(200,{'entries':rows,'sources':[]})
        d=json.loads(self.run_cli('net','2604','--json').stdout)
        self.assertEqual(d['unaccounted'],-4)

    def test_listings_nested_prices_and_positions_snapshot(self):
        self.market()
        listing={'item_id':2604,'name':'Red Dye','count':2,'character':'Slade Undertow',
                 'price':{'available':True,'recorded':{'buyout_copper':200,'expires_at':NOW},'source':{'stale':False}}}
        Handler.routes['/listings']=(200,{'listings':[listing],'scans':[{'available':True,'pricing':{'available':True,'source':{'file':'DataStore_Auctions.lua','scanned_at':NOW,'stale':False}}}],'sources':[]})
        scan_source = Handler.routes['/listings'][1]['scans'][0]['pricing']['source']
        Handler.routes['/listings'][1]['sources'] = [scan_source, dict(scan_source)]
        Handler.routes['/listings'][1]['scans'] *= 2
        d=json.loads(self.run_cli('listings','--json').stdout)
        self.assertEqual(d['sources'].count(scan_source), 1)
        self.assertEqual(d['listings'][0]['buyout_copper'],200)
        self.assertTrue(d['listings'][0]['undercut'])
        self.assertIn('undercut yes', self.run_cli('listings').stdout)
        Handler.routes['/characters']=(200,{'characters':[{'name':'Slade Undertow'}],'sources':[]})
        Handler.routes['/characters/Slade%20Undertow/inventory']=(200,{
            'bags':{'available':True,'items':[{'item_id':2604,'name':'Red Dye','count':4}]},
            'bank':{'available':False,'items':[]},'sources':[]})
        d=json.loads(self.run_cli('positions','--item','2604','--json').stdout)
        self.assertEqual((d['positions'][0]['held'],d['positions'][0]['listed']),(4,2))
        self.assertIsNone(d['positions'][0]['basis_copper'])
        self.assertEqual(d['positions'][0]['value_copper'],348)
        Handler.requests = []
        selected = json.loads(self.run_cli('positions','--char','Slade','--item','2604','--json').stdout)
        self.assertEqual(selected['positions'], d['positions'])
        self.assertEqual([p for p in Handler.requests if p in ('/characters/Slade', '/characters/Slade%20Undertow')],
                         ['/characters/Slade'])
        Handler.routes['/listings'][1]['scans'][0]['pricing']['source']['stale']=True
        self.run_cli('listings','--json',code=3)

    def refresh_routes(self):
        anchors={'tailoring':[("White Swashbuckler's Shirt",125,'Trainer'),('Bolt of Silk Cloth',100,'Trainer'),
            ('Bolt of Woolen Cloth',55,'Trainer'),('Red Woolen Boots',75,'Pattern · Drop'),
            ('Dark Silk Shirt',120,'Pattern · Vendor'),('Robes of Arcana',125,'Pattern · Drop')],
            'leatherworking':[('Cloudy Gustwoven Boots',55,'Trainer')]}
        for prof in ('alchemy','blacksmithing','enchanting','engineering','leatherworking','tailoring',
                     'cooking','first-aid','fishing','mining','herbalism','skinning'):
            rows=anchors.get(prof,[])
            html=f'Recipes ({len(rows)})'
            for index,(name,level,source) in enumerate(rows,1):
                html+=f'<tr role="row"><td role="cell"><span class="o">{level}</span></td><td role="cell"><a href="/professions/{prof}/{index}-recipe/">{name}</a></td><td role="cell"><a href="/items/{index}-item/">{name}</a></td><td role="cell"></td><td role="cell">{source}</td></tr>'
            Handler.routes[f'/professions/{prof}/']=(200,html)

    def test_removed_flags_and_aliases_are_rejected(self):
        for args in [('verify',), ('refresh',), ('margins', '--known', 'Slade'),
                     ('vendors', '2604', '--char', 'Slade'), ('vendors', '2604', '--all')]:
            with self.subTest(args=args):
                self.run_cli(*args, code=2)
        result = self.run_cli('char', 'Slade', 'auctions')
        self.assertNotIn('listings --char', result.stderr)

    def test_refresh_and_status(self):
        self.refresh_routes()
        self.run_cli('vendors','2604','--json')
        Handler.requests=[]
        d=json.loads(self.run_cli('refresh','catalogue','--json').stdout)
        self.assertEqual(d['refreshed'],'catalogue')
        self.assertEqual([r for r in Handler.requests if '.pins.json' in r],['/items/2604.pins.json'])
        self.assertNotIn('/items/6532.json',Handler.requests)
        self.market()
        Handler.routes['/']=(200,{})
        d=json.loads(self.run_cli('status','--json').stdout)
        vendor=next(row for row in d['status'] if row['source']=='vendors')
        self.assertEqual(vendor['backend'],'wowforever.ninja')
        self.assertEqual(vendor['cache_items'],1)
        self.assertEqual(vendor['seen']+vendor['baseline']+vendor['unverified'],89)

    def sync_foreverdb(self):
        self.refresh_routes()
        self.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
        # Only the vendor transport is replaced. The public refresh command still
        # rebuilds the catalogue through the existing local fixture HTTP server.
        script = """
import json, runpy
from pathlib import Path
from unittest.mock import patch
import everledger_foreverdb as db
fixture = json.loads(Path('../tests/fixtures/foreverdb.json').read_text())
def get(url, headers):
    from urllib.parse import urlsplit, parse_qs
    parsed = urlsplit(url)
    if parsed.path == '/api/v1/vendors':
        data = fixture['pages'][int(parse_qs(parsed.query)['offset'][0])//100]
    else:
        data = fixture['details'][parsed.path.rsplit('/',1)[1]]
    return json.dumps(data).encode(), {}
with patch.object(db, 'request', side_effect=get), patch.object(db.time, 'sleep'):
    runpy.run_path('everledger', run_name='__main__')
"""
        result = subprocess.run(['python3', '-c', script, 'refresh', 'catalogue', '--json'],
                                cwd=ROOT, env=self.env, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        return json.loads(result.stdout)

    def test_refresh_sync_failure_reports_catalogue_and_spend_with_or_without_cache(self):
        self.refresh_routes()
        self.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
        script = """
import runpy
from unittest.mock import patch
import everledger_foreverdb as db
from everledger_core import Failure
with patch.object(db, 'request', side_effect=Failure('secret-fixture', 4, {'status':503})):
    runpy.run_path('everledger', run_name='__main__')
"""
        for cached in (False, True):
            if cached:
                self.run_cli('vendors', '2604', code=3)
            for as_json in (False, True):
                result = subprocess.run(['python3', '-c', script, 'refresh', 'catalogue',
                                         *(['--json'] if as_json else [])],
                                        cwd=ROOT, env=self.env, text=True, capture_output=True)
                self.assertEqual(result.returncode, 4, result.stderr + result.stdout)
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

    def test_refresh_setup_failure_preserves_catalogue_result(self):
        self.refresh_routes()
        self.env['EVERLEDGER_FOREVERDB_API_KEY_FILE'] = self.tmp.name
        result = self.run_cli('refresh', 'catalogue', '--json', code=4)
        data = json.loads(result.stdout)
        self.assertEqual(data['refreshed'], 'catalogue')
        self.assertEqual((data['vendor_sync']['failed'], data['vendor_sync']['requests']), (True, 0))
        self.assertIn('API key file unreadable', data['vendor_sync']['error'])
        self.assertEqual(result.stderr, '')

    def test_foreverdb_refresh_selection_credit_and_locations(self):
        self.run_cli('vendors', '2604', '--json')
        ninja = Path(self.tmp.name, 'vendors/wowforever.ninja/2604.json')
        before = ninja.read_bytes()
        sync = self.sync_foreverdb()
        self.assertEqual(sync['vendor_sync']['requests'], 6)
        self.assertEqual(ninja.read_bytes(), before)
        Handler.requests = []
        data = json.loads(self.run_cli('vendors', '2604', '--in', 'IF', '--json').stdout)
        self.assertEqual(data['backend'], 'foreverdb.net')
        self.assertEqual({v['name'] for v in data['vendors']}, {
            'Poranna Snowbraid', 'Outfitter Eric', 'Bombus Finespindle'})
        other = json.loads(self.run_cli('vendors', '2604', '--in', 'Dun Morogh', '--json').stdout)
        self.assertEqual(other['vendors'], [])
        self.assertEqual(data['fallback_additional_sellers'], 1)
        human = self.run_cli('vendors', '2604').stdout
        self.assertIn('Ironforge: 43.4, 29.3 | Alliance', human)
        self.assertIn('sale/price unverified; vendor seen', human)
        filtered = self.run_cli('vendors', '2604', '--in', 'IF').stdout
        self.assertIn('Fallback dataset knows 1 more seller for this item.', filtered)
        self.assertEqual(sum(line.startswith('source:') for line in filtered.splitlines()), 1)
        self.assertIn('source: foreverdb.net', human)
        self.assertNotIn('saved/data unknown', human)
        self.assertNotIn('age unknown', human)
        self.assertIn('Data from ForeverDB.net (https://foreverdb.net)', human)
        self.assertNotIn('each', human)
        unknown = json.loads(self.run_cli('vendors', '999999', '--json').stdout)
        self.assertIsNone(unknown['vendors'])
        self.assertIn('no known sellers in the vendor index', self.run_cli('vendors', '999999').stdout)
        path = Path(self.tmp.name, 'vendors/foreverdb.net/index.json')
        index = json.loads(path.read_text())
        index['items']['9001']['vendors'] = []
        path.write_text(json.dumps(index))
        empty = json.loads(self.run_cli('vendors', '9001', '--json').stdout)
        self.assertEqual(empty['vendors'], [])
        self.assertIsNone(empty['reason'])
        self.assertIn('no known sellers in the vendor index', self.run_cli('vendors', '9001').stdout)
        self.assertEqual(Handler.requests, [])
        self.market()
        self.refresh_routes()
        info = json.loads(self.run_cli('status', '--json').stdout)
        row = next(row for row in info['status'] if row['source'] == 'vendors')
        self.assertEqual(row['backend'], 'foreverdb.net')
        self.assertEqual(row['record_count'], 5)
        self.assertEqual(row['coverage']['wowforever.ninja']['record_count'], 89)
        self.assertEqual(row['coverage']['wowforever.ninja']['item_count'], 1)
        self.assertEqual(row['coverage']['foreverdb.net']['record_count'], 5)
        self.assertEqual(row['coverage']['foreverdb.net']['item_count'], 3)
        self.assertEqual(row['foreverdb']['transport'], 'env-key')
        self.assertIsNotNone(row['last_sync_at'])
        self.assertEqual(row['foreverdb']['state'], 'synced')
        detail = next(source['detail'] for source in info['sources'] if source.get('name') == 'foreverdb.net vendors')
        self.assertIn('3 seen in Forever', detail)
        self.assertIn('0 Classic-only baseline', detail)
        self.assertIn('1 unverified', detail)
        self.assertFalse(any('/api/v1/' in url for url in Handler.requests))
        self.market()
        # Restore the item catalogue for recipe and margin assertions after refresh.
        Path(self.tmp.name, 'catalogue.json').write_bytes((FIX / 'catalogue-fixture.json').read_bytes())
        price = self.run_cli('price', '2604').stdout
        self.assertIn('ForeverDB sell-row labels', price)
        self.assertIn('Data from ForeverDB.net (https://foreverdb.net)', price)
        margins = json.loads(self.run_cli('margins', 'tailoring', '--json').stdout)
        self.assertEqual(margins['margins'][0]['cost_copper'], 88)

    def test_foreverdb_price_displays_uneven_stack_and_limited_stock(self):
        self.sync_foreverdb()
        path = Path(self.tmp.name, 'vendors/foreverdb.net/index.json')
        index = json.loads(path.read_text())
        row = next(r for r in index['items']['2604']['vendors'] if r['npc_id'] == 8681)
        row.update(price_copper=None, stack_price_copper=10, stack=3, limited=True, limitedStockCount=2)
        path.write_text(json.dumps(index))
        self.market()
        output = self.run_cli('price', '2604').stdout
        self.assertIn('10c per 3 | limited stock (2)', output)
        self.assertEqual(sum(line.startswith('source:') for line in output.splitlines()), 1)
        output = self.run_cli('vendors', '2604', '--in', 'IF').stdout
        self.assertIn('10c per 3 | limited stock (2)', output)

    def test_foreverdb_no_key_and_before_sync_status(self):
        self.refresh_routes()
        self.run_cli('refresh', 'catalogue', '--json')
        self.market()
        Handler.routes['/'] = (200, {})
        self.run_cli('vendors', '2604', '--json')
        data = json.loads(self.run_cli('status', '--json').stdout)
        row = next(row for row in data['status'] if row['source'] == 'vendors')
        self.assertEqual(row['backend'], 'wowforever.ninja')
        self.assertEqual(row['foreverdb']['state'], 'not configured')
        self.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'
        data = json.loads(self.run_cli('status', '--json').stdout)
        row = next(row for row in data['status'] if row['source'] == 'vendors')
        self.assertEqual(row['backend'], 'wowforever.ninja')
        self.assertEqual(row['foreverdb']['state'], 'awaiting completed sync')
        self.assertEqual(json.loads(self.run_cli('vendors', '2604', '--json').stdout)['backend'], 'wowforever.ninja')
        self.assertFalse(any('/api/v1/' in url for url in Handler.requests))

    def test_foreverdb_failure_uses_stale_index_then_ninja_and_key_removal(self):
        self.run_cli('vendors', '2604', '--json')
        self.sync_foreverdb()
        directory = Path(self.tmp.name, 'vendors/foreverdb.net')
        attempt = {'attempted_at': NOW, 'requests': 1, 'failed': True, 'status': 503}
        (directory / 'attempt.json').write_text(json.dumps(attempt))
        data = json.loads(self.run_cli('vendors', '2604', '--json', code=3).stdout)
        self.assertEqual(data['backend'], 'foreverdb.net')
        index = json.loads((directory / 'index.json').read_text())
        index['fetched_at'] = '2020-01-01T00:00:00Z'
        (directory / 'index.json').write_text(json.dumps(index))
        data = json.loads(self.run_cli('vendors', '2604', '--json', code=3).stdout)
        self.assertEqual(data['backend'], 'wowforever.ninja')
        index['fetched_at'] = NOW
        (directory / 'index.json').write_text(json.dumps(index))
        attempt['status'] = 401
        (directory / 'attempt.json').write_text(json.dumps(attempt))
        data = json.loads(self.run_cli('vendors', '2604', '--json', code=3).stdout)
        self.assertEqual(data['backend'], 'wowforever.ninja')
        ninja_path = Path(self.tmp.name, 'vendors/wowforever.ninja/2604.json')
        ninja = json.loads(ninja_path.read_text())
        ninja['fetched_at'] = '2020-01-01T00:00:00Z'
        ninja_path.write_text(json.dumps(ninja))
        self.run_cli('vendors', '2604', '--json', code=4)
        ninja['fetched_at'] = NOW
        ninja_path.write_text(json.dumps(ninja))
        del self.env['EVERLEDGER_FOREVERDB_API_KEY']
        data = json.loads(self.run_cli('vendors', '2604', '--json').stdout)
        self.assertEqual(data['backend'], 'wowforever.ninja')

    def test_local_date_and_unknown_inventory(self):
        self.env['TZ']='America/New_York'
        self.run_cli('events','--since','2026-11-14','--until','2026-11-15','--json')
        query=urllib.parse.parse_qs(urllib.parse.urlsplit(Handler.requests[-1]).query)
        self.assertEqual(query['since'],['1794632400'])
        self.assertEqual(query['until'],['1794718800'])

    def test_unknown_vendor_coverage_survives_zone_filter(self):
        Handler.routes['/items/6532.json']=(200,{'id':6532,'name':'Bright Baubles'})
        d=json.loads(self.run_cli('vendors','6532','--in','IF','--json').stdout)
        self.assertIsNone(d['vendors'])
        self.assertEqual(d['reason'],'no pins published')

    def test_usage_errors_and_page_scope(self):
        Handler.routes['/events']=(400,{'error_code':'invalid_query'})
        self.run_cli('events','--type','invalid','--json',code=2)
        Handler.routes['/events']=(200,{'entries':[],'next_cursor':None,'has_more':False,'sources':[]})
        self.run_cli('events','--cursor','opaque','--json')
        self.assertEqual(Handler.requests[-1],'/events?cursor=opaque')
        self.run_cli('events','--cursor','',code=2)

    def test_recipe_data_unavailable_is_not_known_empty(self):
        self.market()
        ch={'name':'Slade Undertow','professions':[{'name':'Tailoring','skill':5,'skill_available':True,
            'recipes':{'available':False,'known':None}}]}
        Handler.routes['/characters/Slade']=(200,{'character':ch,'sources':[]})
        self.run_cli('margins','tailoring','--char','Slade','--json',code=1)
        ch['professions'][0]['recipes']={'available':True,'known':[]}
        d=json.loads(self.run_cli('margins','tailoring','--char','Slade','--json').stdout)
        self.assertEqual(d['margins'],[])

    def test_listing_scan_freshness_is_independent_of_other_roster_files(self):
        self.market()
        Handler.routes['/listings']=(200,{'listings':[], 'sources':[
            {'file':'DataStore_Crafts.lua','mtime':'2020-01-01T00:00:00Z','stale':True}],
            'scans':[{'available':True,'pricing':{'available':True,'source':{
                'file':'DataStore_Auctions.lua','scanned_at':NOW,'stale':False}}}]})
        self.run_cli('listings','--json')
        Handler.routes['/listings'][1]['scans'][0]['pricing']['source']['stale']=True
        self.run_cli('listings','--json',code=3)

    def test_id_listing_queries_keep_nameless_inventory_rows(self):
        self.market()
        row={'item_id':2604,'name':None,'count':2,'price':{'available':True,'recorded':{'buyout_copper':200}}}
        Handler.routes['/listings']=(200,{'listings':[],'sources':[]})
        Handler.routes['/listings?item=2604']=(200,{'listings':[row],'sources':[]})
        d=json.loads(self.run_cli('listings','--item','2604','--json').stdout)
        self.assertEqual(len(d['listings']),1)
        Handler.routes['/events']=(200,{'entries':[self.event('posting',2,-20)],'sources':[]})
        d=json.loads(self.run_cli('net','2604','--json').stdout)
        self.assertEqual(d['listed'],2)
        self.assertEqual(d['unaccounted'],0)
        d=json.loads(self.run_cli('positions','--item','2604','--json').stdout)
        self.assertEqual(d['positions'][0]['listed'],2)

    def test_aggregate_resync_discards_prior_pages_and_keeps_filters(self):
        sale=self.event('sale',1,90,proceeds_copper=90)
        Handler.routes['/events']=(200,{'entries':[sale],'has_more':True,'next_cursor':'bad-page','sources':[{'file':'discarded.lua','file_age_seconds':999}]})
        Handler.routes['/events?limit=500&cursor=bad-page']=(409,{'error_code':'cursor_invalid','resync_since':'2026-11-14T00:00:00Z'})
        Handler.routes['/events?limit=500&since=1794614400']=(200,{
            'entries':[dict(sale,id='restart')],'has_more':False,'next_cursor':'new-terminal','sources':[]})
        r=self.run_cli('sales','--char','Slade','--item','2604','--json')
        d=json.loads(r.stdout)
        self.assertEqual(d['proceeds_copper'],90)
        self.assertNotIn('discarded.lua', json.dumps(d['sources']))
        self.assertEqual([e['id'] for e in d['sales']],['restart'])
        self.assertEqual(r.stderr.count('resynchronizing'),1)

    def test_absent_journal_is_unknown_instead_of_zero_sales(self):
        Handler.routes['/events']=(200,{'available':False,'state':'absent','reason':'Journalator.lua has not been saved',
            'entries':[],'sources':[{'file':'Journalator.lua','mtime':None}],'next_cursor':None,'has_more':False})
        d=json.loads(self.run_cli('sales','--json').stdout)
        self.assertIsNone(d['proceeds_copper'])
        self.assertFalse(d['available'])
        self.assertIn('journal absent',self.run_cli('events').stdout)
        d=json.loads(self.run_cli('net','2604','--json').stdout)
        self.assertIsNone(d['cash_copper'])
        self.assertIsNone(d['sold'])

if __name__ == '__main__':
    unittest.main()
