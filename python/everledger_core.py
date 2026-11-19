import datetime as dt
import http.client
import json
from functools import cache
import os
from pathlib import Path
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

UTC = dt.timezone.utc
DATA_DIR = Path(os.environ.get('EVERLEDGER_DATA_DIR', Path.home() / '.everledger'))
ROSTER = os.environ.get('EVERLEDGER_ROSTER_URL', 'http://localhost:8765').rstrip('/')
AHL = os.environ.get('EVERLEDGER_AHLEDGER_URL', 'https://api.ahledger.com').rstrip('/')
MARKET = os.environ.get('EVERLEDGER_MARKET', 'forever.pvp.alliance.us')

class Failure(Exception):
    def __init__(self, message, code=1, body=None):
        super().__init__(message)
        self.code, self.body = code, body


def now():
    return dt.datetime.now(UTC)


def iso(value):
    return value.astimezone(UTC).isoformat(timespec='seconds').replace('+00:00', 'Z')


def instant(value):
    return dt.datetime.fromisoformat(value.replace('Z', '+00:00')).astimezone(UTC)


def time_selector(value):
    relative = re.fullmatch(r'(\d+)([dh])', value)
    try:
        if relative:
            n, unit = relative.groups()
            return iso(now() - dt.timedelta(**{'days' if unit == 'd' else 'hours': int(n)}))
        return iso(instant(value))
    except (ValueError, OverflowError):
        raise Failure('time must be 7d, 24h, a date, or an ISO timestamp', 2)


def roster_seconds(value):
    if isinstance(value, int) or isinstance(value, str) and value.isdecimal():
        return int(value)
    return int(instant(value).timestamp())


def age(value):
    if value is None:
        return None
    try:
        return max(0, int((now() - instant(value)).total_seconds()))
    except (ValueError, TypeError):
        return None


def money(value):
    if value is None:
        return 'unknown'
    sign = '-' if value < 0 else ''
    g, rem = divmod(abs(round(value)), 10000)
    s, c = divmod(rem, 100)
    parts = [f'{n}{unit}' for n, unit in ((g, 'g'), (s, 's'), (c, 'c')) if n]
    return sign + (' '.join(parts) or '0c')


def request(url, headers=None):
    req = urllib.request.Request(url, headers={'User-Agent': 'everledger', **(headers or {})})
    try:
        try:
            with urllib.request.urlopen(req, timeout=20) as response:
                return response.read(), dict(response.headers)
        except urllib.error.HTTPError as e:
            try:
                body = json.loads(e.read())
            except (ValueError, UnicodeDecodeError):
                body = None
            raise Failure(f'{url}: HTTP {e.code}', 2 if e.code in (400, 422) else 1 if e.code in (404, 409) else 4, {'status': e.code, 'response': body}) from e
    except (urllib.error.URLError, TimeoutError, OSError, http.client.HTTPException) as e:
        raise Failure(f'{url}: source unreachable ({e})', 4) from e


def roster(path, params=None):
    token = os.environ.get('EVERLEDGER_ROSTER_TOKEN', '').strip()
    if not token:
        token_file = os.environ.get('EVERLEDGER_ROSTER_TOKEN_FILE')
        if token_file:
            token = Path(token_file).read_text().strip()
    if not token:
        raise Failure('configure EVERLEDGER_ROSTER_TOKEN or EVERLEDGER_ROSTER_TOKEN_FILE', 4)
    params = dict(params or {})
    if path == '/events':
        for key in ('since', 'until'):
            if params.get(key) is not None:
                params[key] = roster_seconds(params[key])
    query = urllib.parse.urlencode({k: v for k, v in (params or {}).items() if v is not None})
    raw, _ = request(ROSTER + path + ('?' + query if query else ''), {'Authorization': 'Bearer ' + token})
    return json.loads(raw)


def character_path(name):
    return '/characters/' + urllib.parse.quote(name, safe='')


def sources(data):
    value = data.get('sources', [])
    if isinstance(value, dict):
        return [dict(v, name=k) for k, v in value.items() if isinstance(v, dict)]
    return list(value)


def source(name, timestamp=None, **extra):
    return dict(name=name, updated_at=timestamp, age_seconds=age(timestamp), **extra)


def human(value):
    if isinstance(value, bool):
        return 'yes' if value else 'no'
    return 'unknown' if value is None or value == '' else str(value)


def source_lines(items, families=None):
    groups = {}
    for s in items:
        name = s.get('name') or Path(s.get('file') or s.get('path') or 'roster').name
        family = next((label for label, members in (families or {}).items() if s in members), None)
        name = family or name
        when = s.get('saved_at') or s.get('scanned_at') or s.get('mtime') or s.get('updated_at') or s.get('fetched_at')
        seconds = s.get('scan_age_seconds', s.get('file_age_seconds', s.get('age_seconds', age(when))))
        if seconds is None:
            seconds = age(when)
        group = groups.setdefault(name, {'times': set(), 'ages': set(), 'credits': set(), 'files': set(), 'family': family})
        group['files'].add(s.get('file') or s.get('path') or s.get('name') or 'unknown')
        group['times'].add(human(when))
        group['ages'].add(seconds)
        detail = s.get('detail') or ''
        if detail.startswith('Data from '):
            group['credits'].add(detail.split(';', 1)[0])
    summaries = []
    for name, group in groups.items():
        times = sorted(group['times'])
        known_ages = sorted(a for a in group['ages'] if a is not None)
        if group['family']:
            freshness = f'oldest age {known_ages[-1]}s, newest {known_ages[0]}s' if known_ages else 'age unknown'
            if known_ages and None in group['ages']:
                freshness += ', some unknown'
            summaries.append(f"{name} ({len(group['files'])} files; {freshness})")
            continue
        span = str(known_ages[0]) if known_ages else 'unknown'
        if len(known_ages) > 1:
            span += '-' + str(known_ages[-1])
        freshness = span + 's' if known_ages else span
        if known_ages and None in group['ages']:
            freshness += ', some unknown'
        saved = times[0] if len(times) == 1 else times[0] + '..' + times[-1]
        credit = '; '.join(sorted(group['credits']))
        summaries.append(f'{name} (saved/data {saved}; age {freshness})' + (f'; {credit}' if credit else ''))
    print('source: ' + ('; '.join(summaries) or 'unknown (age unknown)'))


def emit(data, lines, as_json, stale=False, source_families=None):
    if as_json:
        print(json.dumps(data, ensure_ascii=False))
    else:
        for line in lines:
            print(line)
        items = sources(data)
        if stale:
            print('stale: data exceeds its freshness limit')
        source_lines(items, source_families)
    return 3 if stale else 0


def is_stale(items):
    return any(s.get('stale', False) for s in items)


@cache
def load_catalogue():
    try:
        cat = json.loads((DATA_DIR / 'catalogue.json').read_text())
        if age(cat['generated_at']) is not None and age(cat['generated_at']) > 14 * 86400:
            print('catalogue is older than 14d; run: everledger refresh catalogue', file=sys.stderr)
        return cat
    except FileNotFoundError:
        raise Failure('catalogue missing; run: everledger refresh catalogue')


def catalogue_source(cat):
    return source(cat.get('source', 'catalogue'), cat['generated_at'])


def recipes(cat):
    for prof, block in cat['professions'].items():
        for row in block['recipes']:
            yield prof, row


def resolve_item(query, cat=None):
    if query.isdigit():
        return {'id': int(query), 'name': None}
    cat = cat or load_catalogue()
    matches = {}
    for _, r in recipes(cat):
        for item in [r['result'], *r['reagents']]:
            if item.get('item_id') and item['name'].casefold() == query.casefold():
                matches[item['item_id']] = item['name']
    if len(matches) > 1:
        raise Failure('ambiguous item: ' + ', '.join(f'{n} ({i})' for i, n in matches.items()), 2)
    if not matches:
        raise Failure('item not found in catalogue; use a numeric item ID')
    iid, name = next(iter(matches.items()))
    return {'id': iid, 'name': name}


def item_matches(row, item):
    recorded = row.get('item') or {}
    iid = recorded.get('id', row.get('item_id'))
    name = recorded.get('name', row.get('name'))
    if iid is not None:
        return iid == item['id']
    return bool(name and item.get('name') and name.casefold() == item['name'].casefold())


def item_basis(row):
    item = row.get('item') or {}
    return 'name' if item.get('id') is None or item.get('id_basis') == 'name' else 'id'


def market_table():
    raw, _ = request(f'{AHL}/v1/pricetable/{MARKET}')
    lines = raw.decode().splitlines()
    head = lines[0].split('|')
    prices = {}
    for line in lines[1:]:
        cells = line.split(':')
        def number(i):
            return float(cells[i]) if cells[i] else None
        prices[int(cells[0])] = {'median_copper': round(number(1)) if number(1) is not None else None,
                                'floor_copper': round(number(2)) if number(2) is not None else None,
                                'quantity': int(number(3)) if number(3) is not None else None, 'sales_per_day': number(8) / 10 if number(8) is not None else None}
    timestamp = iso(dt.datetime.fromtimestamp(int(head[2]), UTC))
    s = source('AHledger ' + MARKET, timestamp)
    s['stale'] = s['age_seconds'] > 900
    return prices, s
