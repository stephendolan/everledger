from pathlib import Path
from copy import deepcopy
from email.utils import parsedate_to_datetime
import json
import os
import re
import sys
import tempfile

from everledger_core import DATA_DIR, Failure, age, iso, now, request, source
import everledger_foreverdb as foreverdb

BACKEND = 'wowforever.ninja'
BASE = os.environ.get('EVERLEDGER_VENDOR_URL', 'https://wowforever.ninja').rstrip('/')
CACHE = DATA_DIR / 'vendors' / BACKEND
ALIASES = {'if': 'ironforge', 'sw': 'stormwind', 'darn': 'darnassus',
           'org': 'orgrimmar', 'tb': 'thunder bluff', 'uc': 'undercity'}
UA = {'User-Agent': 'Mozilla/5.0 (compatible; everledger)'}


def parse_price(text):
    if not re.fullmatch(r'\s*(?:\d+[gsc]\s*)+', text):
        return None
    parts = re.findall(r'(\d+)([gsc])', text)
    if len({unit for _, unit in parts}) != len(parts):
        return None
    return sum(int(n) * {'g': 10000, 's': 100, 'c': 1}[unit] for n, unit in parts)


def normalize(pins):
    rows = []
    for slug, zone in pins.get('zones', {}).items():
        locations = {}
        codes = {}
        code_list = zone.get('codes', {}).get('vendor', [])
        for i, pin in enumerate(zone.get('pins', {}).get('vendor', [])):
            x, y, npc = pin[:3]
            locations.setdefault(str(npc), []).append([x, y])
            if i < len(code_list):
                codes.setdefault(str(npc), code_list[i])
        for npc, vendor in zone.get('vendors', {}).items():
            raw = vendor.get('s', '')
            title, sep, price = raw.rpartition(' · ')
            code = codes.get(str(npc), vendor.get('f'))
            status = {'u': 'unchanged', 'c': 'changed', 'n': 'new', None: 'baseline'}.get(code, 'unverified')
            rows.append({'npc_id': int(npc), 'name': vendor['n'], 'title': title if sep else raw,
                         'zone': zone.get('zone', {}).get('name', slug), 'zone_slug': slug,
                         'coords': locations.get(str(npc), []), 'price_copper': parse_price(price) if sep else None,
                         'price_raw': price if sep else None, 'status_raw': code, 'raw': vendor,
                         'status': status, 'basis': 'seen' if code in ('u', 'c', 'n') else 'baseline' if code is None else 'unverified',
                         'sale_basis': '1.12 baseline', 'price_basis': '1.12 baseline'})
    return sorted(rows, key=lambda row: (row['zone'].casefold(), row['name'].casefold(), row['npc_id']))


def fetch_ninja(item_id):
    def get(suffix):
        try:
            raw, headers = request(f'{BASE}/items/{item_id}.{suffix}', UA)
            return json.loads(raw), headers
        except Failure as e:
            if e.body and e.body.get('status') == 404:
                return None, {}
            raise
    item, item_headers = get('json')
    pins, headers = get('pins.json') if item is not None else (None, {})
    modified = headers.get('Last-Modified') or item_headers.get('Last-Modified')
    published = iso(parsedate_to_datetime(modified)) if modified else None
    return {'item': {'id': item_id, 'name': item.get('name') if item else None},
            'vendors': normalize(pins) if pins is not None else None,
            'reason': 'item not in dataset' if item is None else 'no pins published' if pins is None else None,
            'published_at': published, 'fetched_at': iso(now()),
            'raw_item': item, 'raw_pins': pins, 'backend': BACKEND, 'backend_url': BASE}


def ninja_lookup(item_id, refresh=False):
    path = CACHE / f'{item_id}.json'
    try:
        cached = json.loads(path.read_text())
    except FileNotFoundError:
        cached = None
    if cached and cached.get('backend_url') != BASE:
        cached = None
    stale = False
    if cached and not refresh:
        data = cached
        stale = age(data['fetched_at']) > 86400
    else:
        try:
            data = fetch_ninja(item_id)
        except Failure as e:
            status = (e.body or {}).get('status')
            if not cached or age(cached['fetched_at']) > 7 * 86400 or (status is not None and status < 500):
                raise
            print(f'vendors: {e}; serving stale cache', file=sys.stderr)
            data, stale = cached, True
        else:
            CACHE.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=CACHE, prefix=f'{item_id}.', suffix='.tmp', delete=False) as temporary:
                temporary.write(json.dumps(data, ensure_ascii=False))
            Path(temporary.name).replace(path)
    rows = data['vendors'] or []
    seen = sum(row['basis'] == 'seen' for row in rows)
    baseline = sum(row['basis'] == 'baseline' for row in rows)
    unverified = len(rows) - seen - baseline
    data['stale'] = stale
    data['sources'] = [source(BACKEND + ' pins', data['published_at'], fetched_at=data['fetched_at'], stale=stale,
                              detail=f'1.12 baseline sale/price; {seen} vendor seen in Forever, {baseline} baseline, {unverified} unverified; {len(rows)} vendors, {sum(len(r["coords"]) for r in rows)} pins; faction unavailable')]
    return data


def usable_index(data, attempt):
    status = attempt.get('status')
    return data is not None and not (attempt.get('failed') and
        (age(data['fetched_at']) > 7 * 86400 or (status is not None and status < 500)))


def active_index():
    data = foreverdb.index()
    attempt = (foreverdb.read('attempt.json') or {}) if data else {}
    return data if usable_index(data, attempt) else None


def ninja_fallback(data):
    if age(data['fetched_at']) > 7 * 86400:
        raise Failure('ForeverDB sync failed; ninja fallback cache is older than 7d', 4)
    data['stale'] = True
    for item in data['sources']:
        item['stale'] = True
        item['detail'] += '; ForeverDB sync failed; ninja fallback'
    return data


def lookup(item_id, refresh=False):
    if refresh and foreverdb.api_key():
        refresh_cached()
    data = active_index()
    if data is not None:
        return foreverdb.lookup(item_id, data)
    result = ninja_lookup(item_id, refresh=refresh)
    if foreverdb.api_key() and (foreverdb.read('attempt.json') or {}).get('failed'):
        ninja_fallback(result)
    return result


def refresh_cached():
    failure = None
    try:
        synced = foreverdb.sync()
    except Failure as error:
        failure = error
        synced = {'configured': True, 'complete': False, 'failed': True}
        print(f'{error}; retaining previous vendor caches', file=sys.stderr)
    data = active_index()
    if data is not None:
        return foreverdb.index_sources(data), synced
    paths = sorted(CACHE.glob('*.json'))
    if failure and not paths:
        raise failure
    used = []
    for path in paths:
        result = ninja_lookup(int(path.stem), refresh=True)
        if failure or synced.get('failed'):
            ninja_fallback(result)
        used += result['sources']
    return used, synced


def backend_status():
    configured = bool(foreverdb.api_key())
    completed = foreverdb.index() if configured else None
    attempt = (foreverdb.read('attempt.json') or {}) if configured else {}
    data = completed if usable_index(completed, attempt) else None
    state = 'not configured' if not configured else 'synced' if data else 'awaiting completed sync'
    if attempt.get('failed'):
        state = 'sync failed; serving previous index' if data else 'sync failed; ninja fallback'
    info = {'backend': foreverdb.BACKEND, 'state': state,
            'last_sync_at': completed['fetched_at'] if completed else None,
            'record_count': completed['record_count'] if completed else 0,
            'item_count': completed['item_count'] if completed else 0,
            'last_attempt_at': attempt.get('attempted_at'),
            'last_attempt_requests': attempt.get('requests', 0),
            'pending': foreverdb.read('pending.json') is not None if configured else False}
    if data is not None:
        rows = [row for item in data['items'].values() for row in item['vendors']]
        summary = {'source': 'vendors', 'backend': foreverdb.BACKEND, 'state': state,
                   'cache_items': data['item_count'], 'published_at': data['published_at'],
                   'last_sync_at': data['fetched_at'], 'record_count': data['record_count'],
                   **{basis: sum(row['basis'] == basis for row in rows)
                      for basis in ('seen', 'baseline', 'unverified')}, 'foreverdb': info}
        return summary, foreverdb.index_sources(data)
    cached = [json.loads(path.read_text()) for path in CACHE.glob('*.json')]
    rows = [row for item in cached for row in item.get('vendors') or []]
    summary = {'source': 'vendors', 'backend': BACKEND, 'cache_items': len(cached),
               'published_at': max((item.get('published_at') or '' for item in cached), default=None),
               **{basis: sum(row['basis'] == basis for row in rows)
                  for basis in ('seen', 'baseline', 'unverified')}, 'foreverdb': info}
    return summary, [source(BACKEND, summary['published_at'],
                            detail=f'cache {len(cached)} items; ForeverDB {state}')]


def public(data):
    return {k: v for k, v in data.items() if k not in ('raw_item', 'raw_pins', 'backend_url')}


def select_zone(data, query):
    data = deepcopy(data)
    zones = sorted({p['zone'] for v in data['vendors'] or [] for p in v.get('locations', [v])})
    q = ALIASES.get(query.casefold(), query.casefold())
    if data['vendors'] is not None:
        data['vendors'] = [v for v in data['vendors'] if any(q in p['zone'].casefold() or q in p['zone_slug'].casefold()
                                        for p in v.get('locations', [v]))]
    data['known_zones'] = zones
    return data


def lines(data):
    rows = data['vendors']
    if not rows:
        message = 'vendor coverage unknown on wowforever.ninja' if data.get('reason') == 'no pins published' else 'no known sellers in the vendor index'
        result = [message]
        if data.get('known_zones'):
            result.append('known zones: ' + ', '.join(data['known_zones']))
        return result
    claim = 'sale/price from ForeverDB sell-row labels' if data['backend'] == foreverdb.BACKEND else 'sale and price: 1.12 baseline'
    result = [f"{data['item']['name'] or data['item']['id']} — vendor listed prices ({claim}; faction unavailable)"]
    from everledger_core import money
    for v in rows:
        if data['backend'] == foreverdb.BACKEND:
            locations = '; '.join(f"{p['zone']}: {p.get('x')}, {p.get('y')}" for p in v['locations']) or 'location unknown'
            result.append(f"  {v['name']} ({v['title']}) | {locations} | listed price {money(v['price_copper'])} | sale/price {v['sale_basis']}; vendor {v['vendor_basis']}")
            continue
        coords = ', '.join(str(n) for n in v['coords'][0]) if v['coords'] else 'coordinates unknown'
        extra = f" (+{len(v['coords']) - 1} locations)" if len(v['coords']) > 1 else ''
        basis = {'seen': 'vendor seen in Forever', 'baseline': '1.12 baseline', 'unverified': 'unverified'}[v['basis']]
        result.append(f"  {v['zone']}: {v['name']} ({v['title']}) {coords}{extra} | listed price {money(v['price_copper'])} | {basis}")
    return result
