from pathlib import Path
from copy import deepcopy
from email.utils import parsedate_to_datetime
import json
import os
import re
import sys
import tempfile

from everledger_core import DATA_DIR, Failure, age, iso, now, request, source

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


def lookup(item_id, refresh=False):
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


def public(data):
    return {k: v for k, v in data.items() if k not in ('raw_item', 'raw_pins', 'backend_url')}


def select_zone(data, query):
    data = deepcopy(data)
    zones = sorted({v['zone'] for v in data['vendors'] or []})
    q = ALIASES.get(query.casefold(), query.casefold())
    if data['vendors'] is not None:
        data['vendors'] = [v for v in data['vendors'] if q in v['zone'].casefold() or q in v['zone_slug'].casefold()]
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
    result = [f"{data['item']['name'] or data['item']['id']} — vendor listed prices (sale and price: 1.12 baseline; faction unavailable)"]
    from everledger_core import money
    for v in rows:
        coords = ', '.join(str(n) for n in v['coords'][0]) if v['coords'] else 'coordinates unknown'
        extra = f" (+{len(v['coords']) - 1} locations)" if len(v['coords']) > 1 else ''
        basis = {'seen': 'vendor seen in Forever', 'baseline': '1.12 baseline', 'unverified': 'unverified'}[v['basis']]
        result.append(f"  {v['zone']}: {v['name']} ({v['title']}) {coords}{extra} | listed price {money(v['price_copper'])} | {basis}")
    return result
