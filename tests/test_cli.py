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
FIX = ROOT / 'tests/fixtures'
NOW = datetime.datetime.now(datetime.UTC).isoformat().replace('+00:00', 'Z')

class Handler(http.server.BaseHTTPRequestHandler):
    routes = {}
    requests = []

    def do_GET(self):
        self.requests.append(self.path)
        status, payload = self.routes.get(self.path, self.routes.get(self.path.split('?')[0], (404, {})))
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
        self.env.pop('EVERLEDGER_FOREVERDB_API_KEY', None)
        self.env.pop('EVERLEDGER_FOREVERDB_API_KEY_FILE', None)
        Handler.requests = []
        Handler.routes = {
            '/items/2604.json': (200, json.loads((FIX / '2604.json').read_text())),
            '/items/2604.pins.json': (200, json.loads((FIX / '2604.pins.json').read_text())),
            '/events': (200, {'events': [], 'next_cursor':'opaque+/=', 'has_more':False,
                               'sources':[{'file':'Journalator.lua','mtime':'2020-01-01T00:00:00Z','stale':True}]}),
            '/listings': (200, {'listings':[], 'sources':[]}),
            '/characters': (200, {'characters':[], 'sources':[]}),
        }

    def run_cli(self, *args, code=0):
        r = subprocess.run([str(ROOT/'everledger'), *args], env=self.env, text=True, capture_output=True)
        self.assertEqual(r.returncode, code, r.stderr + r.stdout)
        return r

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
        self.assertEqual(d['next_cursor'],'opaque+/=')
        self.assertIs(d['has_more'],False)
        self.run_cli('events','--after','opaque+/=','--char','Slade','--item','Red Dye','--venue','vendor','--type','sale','--json')
        self.assertIn('after=opaque%2B%2F%3D',Handler.requests[-1])
        self.assertIn('char=Slade',Handler.requests[-1])
        for args in [('events','--after',''),('events','--after','x','--until','7d'),('sales','--since','bad'),('price',),('margins','tailoring','12')]:
            self.run_cli(*args, code=2)
        self.assertIn('events --limit 0 --json',self.run_cli('events','--after','',code=2).stderr)

    def test_outcomes_are_not_stale_errors(self):
        r=self.run_cli('sales','--since','7d','--until','2026-11-14','--json')
        self.assertEqual(json.loads(r.stdout)['sales'],[])
        self.assertIn('venue=auction',Handler.requests[-1])
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
        self.assertEqual(query['char'],['Slade Undertow'])
        self.assertTrue(query['since'][0].endswith('Z'))
        self.assertEqual(query['until'],['2099-01-01T00:00:00Z'])
        for bad in [('events','--limit','-1'),('sales','--since','2026-11-15','--until','2026-11-14'),
                    ('purchases','--venue','both'),('char','Slade','auctions')]:
            self.run_cli(*bad,code=2)

    def test_sales_proceeds_and_linkless_basis(self):
        sale=self.event('sale',4,38400,value_copper=40000,proceeds_copper=38000,
                        item={'id':None,'name':'rEd dYe'})
        Handler.routes['/events']=(200,{'events':[sale],'sources':[{'file':'Journalator.lua','mtime':'2020-01-01T00:00:00Z','stale':True}]})
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
        self.assertIn('since=2026-11-14T00%3A00%3A00Z',Handler.requests[-1])
        self.assertEqual(json.loads(r.stdout)['next_cursor'],'opaque+/=')
        self.run_cli('events','--cursor','page','--since','7d',code=2)

    def test_price_margins_and_required_source_exit_codes(self):
        self.market()
        d=json.loads(self.run_cli('price','2604','--json').stdout)
        self.assertEqual(d['latest']['median_copper'],88)
        self.assertEqual(d['vendors'][0]['price_copper'],50)
        self.assertIn('vendor: listed price 50c',self.run_cli('price','Red Dye').stdout)
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
        script = "import everledger_vendors as v; from everledger_core import Failure; import sys;\ntry:\n d=v.lookup(2604,refresh=True); print(d['stale']); sys.exit(3 if d['stale'] else 0)\nexcept Failure as e: sys.exit(e.code)"
        Handler.routes['/items/2604.json']=(503,{})
        r=subprocess.run(['python3','-c',script],cwd=ROOT,env=self.env,capture_output=True,text=True)
        self.assertEqual(r.returncode,3,r.stderr)
        cached['fetched_at']=(datetime.datetime.now(datetime.UTC)-datetime.timedelta(days=8)).isoformat()
        path.write_text(json.dumps(cached))
        r=subprocess.run(['python3','-c',script],cwd=ROOT,env=self.env,capture_output=True,text=True)
        self.assertEqual(r.returncode,4,r.stderr)
        self.assertEqual(r.stdout,'')

    def test_recipe_bridge_char_and_alias_regressions(self):
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
        r=self.run_cli('margins','tailoring','--known','Slade','--json')
        self.assertIn('deprecated',r.stderr)
        self.assertEqual(len(json.loads(r.stdout)['margins']),1)

    def test_net_signed_counts_and_unknown_profit(self):
        rows=[self.event('posting',6,-600),self.event('sale',4,38400,proceeds_copper=38000),self.event('expiration',2,0)]
        Handler.routes['/events']=(200,{'events':rows,'available':True,'sources':[]})
        d=json.loads(self.run_cli('net','Red Dye','--json').stdout)
        self.assertEqual((d['unaccounted'],d['cash_copper'],d['proceeds_copper']),(0,37800,38000))
        self.assertIsNone(d['realized_profit_copper'])
        self.assertIsNone(d['deposits_lost_copper'])
        Handler.routes['/events']=(200,{'events':rows[1:],'sources':[]})
        r=self.run_cli('net','Red Dye')
        self.assertIn('posted unknown',r.stdout)
        self.assertNotIn('deposits lost',r.stdout)
        rows[0]['count']=2
        Handler.routes['/events']=(200,{'events':rows,'sources':[]})
        d=json.loads(self.run_cli('net','2604','--json').stdout)
        self.assertEqual(d['unaccounted'],-4)

    def test_listings_nested_prices_and_positions_snapshot(self):
        self.market()
        listing={'item_id':2604,'name':'Red Dye','count':2,'character':'Slade Undertow',
                 'price':{'available':True,'recorded':{'buyout_copper':200,'expires_at':NOW},'source':{'stale':False}}}
        Handler.routes['/listings']=(200,{'listings':[listing],'scans':[{'available':True,'pricing':{'available':True,'source':{'file':'DataStore_Auctions.lua','scanned_at':NOW,'stale':False}}}],'sources':[]})
        d=json.loads(self.run_cli('listings','--json').stdout)
        self.assertEqual(d['listings'][0]['buyout_copper'],200)
        self.assertTrue(d['listings'][0]['undercut'])
        Handler.routes['/characters']=(200,{'characters':[{'name':'Slade Undertow'}],'sources':[]})
        Handler.routes['/characters/Slade%20Undertow/inventory']=(200,{
            'bags':{'available':True,'items':[{'item_id':2604,'name':'Red Dye','count':4}]},
            'bank':{'available':False,'items':[]},'sources':[]})
        d=json.loads(self.run_cli('positions','--item','2604','--json').stdout)
        self.assertEqual((d['positions'][0]['held'],d['positions'][0]['listed']),(4,2))
        self.assertIsNone(d['positions'][0]['basis_copper'])
        self.assertEqual(d['positions'][0]['value_copper'],348)
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

    def test_refresh_verify_aliases_and_status(self):
        self.refresh_routes()
        self.run_cli('vendors','2604','--json')
        Handler.requests=[]
        d=json.loads(self.run_cli('refresh','catalogue','--json').stdout)
        self.assertEqual(d['refreshed'],'catalogue')
        self.assertEqual([r for r in Handler.requests if '.pins.json' in r],['/items/2604.pins.json'])
        self.assertNotIn('/items/6532.json',Handler.requests)
        r=self.run_cli('verify','--json')
        self.assertTrue(json.loads(r.stdout)['ok'])
        self.assertIn('deprecated verify',r.stderr)
        r=self.run_cli('refresh','--json')
        self.assertEqual(json.loads(r.stdout)['refreshed'],'catalogue')
        self.assertIn('deprecated bare refresh',r.stderr)
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
fixture = json.loads(Path('tests/fixtures/foreverdb.json').read_text())
def get(url, headers):
    from urllib.parse import urlsplit, parse_qs
    parsed = urlsplit(url)
    if parsed.path == '/api/v1/vendors':
        data = fixture['pages'][int(parse_qs(parsed.query)['page'][0])-1]
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
            'Poranna Snowbraid', 'Outfitter Eric', 'Burbik Gearspanner', 'Bombus Finespindle'})
        other = json.loads(self.run_cli('vendors', '2604', '--in', 'Dun Morogh', '--json').stdout)
        self.assertEqual([v['name'] for v in other['vendors']], ['Burbik Gearspanner'])
        self.assertEqual(len(other['vendors'][0]['locations']), 2)
        human = self.run_cli('vendors', '2604').stdout
        self.assertIn('Dun Morogh: 48.1, 59.2', human)
        self.assertIn('sale/price Classic-only baseline; vendor seen', human)
        self.assertIn('source: foreverdb.net', human)
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
        self.assertEqual(row['record_count'], 4)
        self.assertIsNotNone(row['last_sync_at'])
        self.assertEqual(row['foreverdb']['state'], 'synced')
        detail = next(source['detail'] for source in info['sources'] if source.get('name') == 'foreverdb.net vendors')
        self.assertIn('3 seen in Forever', detail)
        self.assertIn('1 Classic-only baseline', detail)
        self.assertFalse(any('/api/v1/' in url for url in Handler.requests))
        self.market()
        # Restore the item catalogue for recipe and margin assertions after refresh.
        Path(self.tmp.name, 'catalogue.json').write_bytes((FIX / 'catalogue-fixture.json').read_bytes())
        price = self.run_cli('price', '2604').stdout
        self.assertIn('ForeverDB sell-row labels', price)
        self.assertIn('Data from ForeverDB.net (https://foreverdb.net)', price)
        margins = json.loads(self.run_cli('margins', 'tailoring', '--json').stdout)
        self.assertEqual(margins['margins'][0]['cost_copper'], 88)

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
        self.assertEqual(query['since'],['2026-11-14T05:00:00Z'])
        self.assertEqual(query['until'],['2026-11-15T05:00:00Z'])

    def test_unknown_vendor_coverage_survives_zone_filter(self):
        Handler.routes['/items/6532.json']=(200,{'id':6532,'name':'Bright Baubles'})
        d=json.loads(self.run_cli('vendors','6532','--in','IF','--json').stdout)
        self.assertIsNone(d['vendors'])
        self.assertEqual(d['reason'],'no pins published')

    def test_usage_errors_and_page_scope(self):
        Handler.routes['/events']=(400,{'error_code':'invalid_query'})
        self.run_cli('events','--type','invalid','--json',code=2)
        Handler.routes['/events']=(200,{'events':[],'next_cursor':None,'has_more':False,'sources':[]})
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
        row={'item_id':2604,'name':None,'count':2,'available':True,'buyout_copper':200}
        Handler.routes['/listings']=(200,{'listings':[],'sources':[]})
        Handler.routes['/listings?item=2604']=(200,{'listings':[row],'sources':[]})
        d=json.loads(self.run_cli('listings','--item','2604','--json').stdout)
        self.assertEqual(len(d['listings']),1)
        Handler.routes['/events']=(200,{'events':[self.event('posting',2,-20)],'sources':[]})
        d=json.loads(self.run_cli('net','2604','--json').stdout)
        self.assertEqual(d['listed'],2)
        self.assertEqual(d['unaccounted'],0)
        d=json.loads(self.run_cli('positions','--item','2604','--json').stdout)
        self.assertEqual(d['positions'][0]['listed'],2)

    def test_aggregate_resync_discards_prior_pages_and_keeps_filters(self):
        sale=self.event('sale',1,90,proceeds_copper=90)
        Handler.routes['/events']=(200,{'events':[sale],'has_more':True,'next_cursor':'bad-page','sources':[]})
        Handler.routes['/events?limit=500&cursor=bad-page']=(409,{'error_code':'cursor_invalid','resync_since':'2026-11-14T00:00:00Z'})
        Handler.routes['/events?char=Slade&venue=auction&type=sale&limit=500&since=2026-11-14T00%3A00%3A00Z&item=Red+Dye']=(200,{
            'events':[dict(sale,id='restart')],'has_more':False,'next_cursor':'new-terminal','sources':[]})
        r=self.run_cli('sales','--char','Slade','--item','2604','--json')
        d=json.loads(r.stdout)
        self.assertEqual(d['proceeds_copper'],90)
        self.assertEqual([e['id'] for e in d['sales']],['restart'])
        self.assertEqual(r.stderr.count('resynchronizing'),1)

    def test_absent_journal_is_unknown_instead_of_zero_sales(self):
        Handler.routes['/events']=(200,{'available':False,'state':'absent','reason':'Journalator.lua has not been saved',
            'events':[],'sources':[{'file':'Journalator.lua','mtime':None}],'next_cursor':None,'has_more':False})
        d=json.loads(self.run_cli('sales','--json').stdout)
        self.assertIsNone(d['proceeds_copper'])
        self.assertFalse(d['available'])
        self.assertIn('journal absent',self.run_cli('events').stdout)
        d=json.loads(self.run_cli('net','2604','--json').stdout)
        self.assertIsNone(d['cash_copper'])
        self.assertIsNone(d['sold'])

if __name__ == '__main__':
    unittest.main()
