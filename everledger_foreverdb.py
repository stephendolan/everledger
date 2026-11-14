"""Vendor-only, checkpointed ForeverDB sync; no request occurs during lookup/status."""
import fcntl
import json
import os
from pathlib import Path
import tempfile
import time
import urllib.parse
import uuid

from everledger_core import DATA_DIR, Failure, age, iso, now, request, source
import everledger_foreverdb_mapping as mapping

BACKEND = 'foreverdb.net'
BASE = 'https://foreverdb.net'
CACHE = DATA_DIR / 'vendors' / BACKEND
DAILY_REQUESTS = 36
INTERVAL = 2.1  # At most 29 request starts in any 60-second window.
CREDIT = 'Data from ForeverDB.net (https://foreverdb.net)'


def api_key():
    key = os.environ.get('EVERLEDGER_FOREVERDB_API_KEY', '').strip()
    if key:
        return key
    path = Path(os.environ.get('EVERLEDGER_FOREVERDB_API_KEY_FILE') or DATA_DIR / 'foreverdb-api-key')
    try:
        return path.read_text().strip()
    except FileNotFoundError:
        return ''
    except (OSError, UnicodeError):
        raise Failure('ForeverDB API key file unreadable', 4) from None


def read(name):
    try:
        return json.loads((CACHE / name).read_text())
    except FileNotFoundError:
        return None


def write(name, data):
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=CACHE,
                                         prefix=name + '.', suffix='.tmp', delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(data, stream, ensure_ascii=False)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(CACHE / name)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def index():
    return read('index.json') if api_key() else None


def snapshot(pending, previous):
    items = {key: {'item': value['item'], 'vendors': []}
             for key, value in (previous or {}).get('items', {}).items()}
    for detail in pending['details'].values():
        for sale in detail['data']['sells']:
            key = str(sale['item']['id'])
            entry = items.setdefault(key, {'item': sale['item'], 'vendors': []})
            entry['vendors'].append(sale['row'])
    for entry in items.values():
        entry['vendors'].sort(key=lambda row: (row['zone'].casefold(), row['name'].casefold(), row['npc_id']))
    return {'backend': BACKEND, 'fetched_at': iso(now()), **pending['metadata'],
            'record_count': len(pending['details']), 'item_count': len(items),
            'items': items, 'details': pending['details'],
            'sync_requests': pending['total_requests']}


def sync():
    key = api_key()
    if not key:
        return {'configured': False, 'complete': False, 'requests': 0}
    CACHE.mkdir(parents=True, exist_ok=True)
    with (CACHE / 'sync.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        return _sync(key)


def _sync(key):
    previous = read('index.json')
    attempt = read('attempt.json')
    pending = read('pending.json')
    if attempt and age(attempt.get('finished_at') or attempt.get('last_request_at') or attempt['attempted_at']) < 86400:
        return {'configured': True, 'complete': previous is not None and pending is None,
                'pending': pending is not None, 'requests': 0,
                'deferred': 'daily sync limit', 'failed': attempt.get('failed', False)}
    if pending and previous and pending['sync_id'] == previous.get('sync_id'):
        (CACHE / 'pending.json').unlink()
        pending = None
    if pending is None:
        pending = {'sync_id': uuid.uuid4().hex, 'started_at': iso(now()), 'next_page': 1, 'revisions': {},
                   'details': {}, 'metadata': None, 'total_requests': 0}
    if pending['sync_id'] == (attempt or {}).get('sync_id'):
        pending['total_requests'] = max(pending['total_requests'], attempt.get('total_requests', 0))
    attempt = {'attempted_at': iso(now()), 'requests': 0, 'sync_id': pending['sync_id'],
               'failed': (attempt or {}).get('failed', False),
               'status': (attempt or {}).get('status')}
    write('attempt.json', attempt)
    last_start = None
    obsolete = False

    def get(path):
        nonlocal last_start
        if last_start is not None:
            time.sleep(max(0, INTERVAL - (time.monotonic() - last_start)))
        attempt['requests'] += 1
        pending['total_requests'] += 1
        attempt['total_requests'] = pending['total_requests']
        attempt['last_request_at'] = iso(now())
        write('attempt.json', attempt)
        last_start = time.monotonic()
        try:
            raw, _ = request(BASE + path, {'X-API-Key': key})
        except (Failure, OSError, ValueError) as error:
            status = (error.body or {}).get('status') if isinstance(error, Failure) else None
            raise Failure('ForeverDB sync failed' + (f' (HTTP {status})' if status else ''),
                          4, {'status': status}) from None
        try:
            return json.loads(raw)
        except (ValueError, UnicodeError):
            mapping.invalid()

    try:
        while pending['next_page'] is not None and attempt['requests'] < DAILY_REQUESTS:
            number = pending['next_page']
            payload = get('/api/v1/vendors?' + urllib.parse.urlencode({'page': number, 'per_page': 100}))
            revisions, following, metadata = mapping.page(payload, number)
            if pending['metadata'] is not None and pending['metadata'] != metadata:
                obsolete = True
                mapping.invalid()
            if pending['revisions'].keys() & revisions.keys():
                mapping.invalid()
            pending['metadata'] = metadata
            pending['revisions'].update(revisions)
            pending['next_page'] = following
            write('pending.json', pending)
        if pending['next_page'] is None:
            for vendor_id, revision in pending['revisions'].items():
                if vendor_id in pending['details']:
                    continue
                old = (previous or {}).get('details', {}).get(vendor_id)
                if old and old['revision'] == revision:
                    pending['details'][vendor_id] = old
                elif attempt['requests'] < DAILY_REQUESTS:
                    data = mapping.vendor(get('/api/v1/vendors/' + vendor_id), int(vendor_id))
                    pending['details'][vendor_id] = {'revision': revision, 'data': data}
                else:
                    break
                write('pending.json', pending)
        complete = pending['next_page'] is None and len(pending['details']) == len(pending['revisions'])
        if complete:
            data = snapshot(pending, previous)
            data['sync_id'] = pending['sync_id']
            write('index.json', data)
            (CACHE / 'pending.json').unlink(missing_ok=True)
            attempt['failed'] = False
            attempt['status'] = None
        else:
            write('pending.json', pending)
        attempt['finished_at'] = iso(now())
        write('attempt.json', attempt)
        return {'configured': True, 'complete': complete, 'pending': not complete,
                'requests': attempt['requests'], 'total_requests': pending['total_requests']}
    except (Failure, OSError, KeyboardInterrupt) as error:
        attempt['finished_at'] = iso(now())
        attempt['failed'] = True
        attempt['status'] = (error.body or {}).get('status') if isinstance(error, Failure) else None
        write('attempt.json', attempt)
        if obsolete:
            (CACHE / 'pending.json').unlink(missing_ok=True)
        else:
            write('pending.json', pending)
        raise


def lookup(item_id, data=None):
    data = data if data is not None else index()
    if data is None:
        raise Failure('ForeverDB index unavailable; run: everledger refresh catalogue', 4)
    entry = data['items'].get(str(item_id))
    rows = entry['vendors'] if entry else None
    attempt = read('attempt.json') or {}
    stale = age(data['fetched_at']) > 86400 or attempt.get('failed', False)
    return {'item': entry['item'] if entry else {'id': item_id, 'name': None},
            'vendors': rows, 'reason': None if entry else 'item not in dataset',
            'backend': BACKEND, 'published_at': data['published_at'],
            'fetched_at': data['fetched_at'], 'stale': stale,
            'sources': provenance(data, rows or [], stale)}


def provenance(data, rows, stale):
    counts = {basis: sum(row['basis'] == basis for row in rows)
              for basis in ('seen', 'baseline', 'unverified')}
    return [source(BACKEND + ' vendors', data['published_at'],
                   fetched_at=data['fetched_at'], stale=stale,
                   detail=f'{CREDIT}; sell-row labels: {counts["seen"]} seen in Forever, '
                   f'{counts["baseline"]} Classic-only baseline, {counts["unverified"]} unverified; '
                   'vendor sightings separate; listed copper prices; faction unavailable')]


def index_sources(data):
    rows = [row for item in data['items'].values() for row in item['vendors']]
    attempt = read('attempt.json') or {}
    return provenance(data, rows, age(data['fetched_at']) > 86400 or attempt.get('failed', False))
