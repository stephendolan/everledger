"""Vendor-only, checkpointed ForeverDB sync; no request occurs during lookup/status."""
import datetime as dt
import fcntl
import importlib
import json
import math
import os
from pathlib import Path
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

from everledger_core import DATA_DIR, Failure, age, iso, now, instant, source
import everledger_foreverdb_mapping as mapping

BACKEND = 'foreverdb.net'
BASE = 'https://foreverdb.net'
CACHE = DATA_DIR / 'vendors' / BACKEND
try:
    DAILY_REQUESTS = int(os.environ.get('EVERLEDGER_FOREVERDB_DAILY_REQUESTS', '36'))
    if DAILY_REQUESTS <= 0:
        raise ValueError
except ValueError:
    raise Failure('EVERLEDGER_FOREVERDB_DAILY_REQUESTS must be a positive integer', 2) from None
INTERVAL = 2.1  # At most 29 request starts in any 60-second window.
FORMAT = 2
USER_AGENT = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36'
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


def transport():
    if os.environ.get('EVERLEDGER_FOREVERDB_VIA_CONNECTOR', '').strip():
        return 'connector-surrogate'
    return 'env-key' if api_key() else None


def connector_helper():
    directory = os.environ.get('EVERLEDGER_DYNAMIC_CREDENTIALS_DIR', '/opt/muse/skills/custom-connector-setup/bin')
    sys.path.insert(0, directory)
    try:
        module = importlib.import_module('dynamic_credentials')
        return module.add_surrogate_to_request
    except (ImportError, AttributeError):
        raise Failure('ForeverDB connector mode requires dynamic_credentials.add_surrogate_to_request in EVERLEDGER_DYNAMIC_CREDENTIALS_DIR', 4) from None
    except Exception:
        raise Failure('ForeverDB connector helper could not be loaded', 4) from None
    finally:
        sys.path.pop(0)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(url, headers):
    req = urllib.request.Request(url, headers={**headers, 'User-Agent': USER_AGENT})
    slug = os.environ.get('EVERLEDGER_FOREVERDB_VIA_CONNECTOR', '').strip()
    if slug:
        try:
            modified = connector_helper()(req, slug, allowed_hosts=['foreverdb.net'])
            if modified is not None:
                req = modified
        except Exception:
            raise Failure('ForeverDB connector request preparation failed', 4) from None
    req.add_header('User-Agent', USER_AGENT)
    try:
        with urllib.request.build_opener(NoRedirect()).open(req, timeout=20) as response:
            return response.read(), dict(response.headers)
    except urllib.error.HTTPError as error:
        try:
            body = json.loads(error.read())
        except (ValueError, UnicodeError):
            body = None
        raise Failure('ForeverDB HTTP error', 4, {'status': error.code, 'response': body}) from None
    except (urllib.error.URLError, OSError):
        raise Failure('ForeverDB transport failed', 4) from None


class BudgetExhausted(Exception):
    pass


def prune_spends(attempt):
    attempt['spends'] = [spend for spend in attempt['spends'] if age(spend['finished_at']) < 86400]


def read(name):
    try:
        data = json.loads((CACHE / name).read_text())
    except FileNotFoundError:
        return None
    if name == 'attempt.json':
        if 'spends' not in data or not data.get('finished_at'):
            finished = data.get('finished_at') or data.get('last_request_at') or data['attempted_at']
            data.setdefault('spends', []).append({'requests': data.get('requests', 0), 'finished_at': finished})
        prune_spends(data)
    return data


def write(name, data):
    if name == 'attempt.json':
        prune_spends(data)
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
    data = read('index.json') if transport() else None
    return data if data and data.get('format') == FORMAT else None


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
    return {'format': FORMAT, 'backend': BACKEND, 'fetched_at': iso(now()), **pending['metadata'],
            'record_count': len(pending['details']), 'item_count': len(items),
            'items': items, 'details': pending['details'],
            'sync_requests': pending['total_requests']}


def sync():
    mode = transport()
    if not mode:
        return {'configured': False, 'complete': False, 'requests': 0}
    key = api_key() if mode == 'env-key' else None
    CACHE.mkdir(parents=True, exist_ok=True)
    with (CACHE / 'sync.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        return _sync(key)


def _sync(key):
    previous = index()
    attempt = read('attempt.json')
    pending = read('pending.json')
    spends = (attempt or {}).get('spends', [])
    budget = DAILY_REQUESTS - sum(spend['requests'] for spend in spends)
    if budget <= 0:
        return {'configured': True, 'complete': previous is not None and pending is None,
                'pending': pending is not None, 'requests': 0,
                'deferred': 'daily sync limit', 'failed': attempt.get('failed', False)}
    if pending and pending.get('format') != FORMAT:
        pending = None
    if pending and previous and pending['sync_id'] == previous.get('sync_id'):
        (CACHE / 'pending.json').unlink()
        pending = None
    if pending is None:
        pending = {'format': FORMAT, 'sync_id': uuid.uuid4().hex, 'started_at': iso(now()), 'next_offset': 0, 'vendor_ids': [],
                   'details': {}, 'metadata': None, 'total_requests': 0}
    if pending['sync_id'] == (attempt or {}).get('sync_id'):
        pending['total_requests'] = max(pending['total_requests'], attempt.get('total_requests', 0))
    attempt = {'attempted_at': iso(now()), 'requests': 0, 'sync_id': pending['sync_id'], 'spends': spends,
               'failed': (attempt or {}).get('failed', False),
               'status': (attempt or {}).get('status')}
    write('attempt.json', attempt)
    last_start = None
    obsolete = False

    def get(path):
        nonlocal last_start
        while True:
            if attempt['requests'] >= budget:
                raise BudgetExhausted()
            retry = pending.get('retry')
            if retry:
                time.sleep(max(0, (instant(retry['not_before']) - now()).total_seconds()))
            if last_start is not None:
                time.sleep(max(0, INTERVAL - (time.monotonic() - last_start)))
            attempt['requests'] += 1
            pending['total_requests'] += 1
            attempt['total_requests'] = pending['total_requests']
            attempt['last_request_at'] = iso(now())
            write('attempt.json', attempt)
            last_start = time.monotonic()
            try:
                headers = {'User-Agent': USER_AGENT}
                if key:
                    headers['X-API-Key'] = key
                raw, _ = request(BASE + path, headers)
            except (Failure, OSError, ValueError) as error:
                body = (error.body or {}) if isinstance(error, Failure) else {}
                status = body.get('status')
                response = body.get('response')
                if status == 502 and not retry and isinstance(response, dict) and response.get('retryable') is True:
                    delay = response.get('retry_after', 60)
                    if type(delay) not in (int, float) or not math.isfinite(delay) or delay < 0:
                        delay = 60
                    pending['retry'] = {'not_before': iso(now() + dt.timedelta(seconds=math.ceil(max(60, delay)) + 1))}
                    write('pending.json', pending)
                    continue
                pending.pop('retry', None)
                raise Failure('ForeverDB sync failed' + (f' (HTTP {status})' if status else ''),
                              4, {'status': status}) from None
            pending.pop('retry', None)
            try:
                return json.loads(raw)
            except (ValueError, UnicodeError):
                mapping.invalid()

    try:
        if key is None:
            connector_helper()
        try:
            while pending['next_offset'] is not None and attempt['requests'] < budget:
                offset = pending['next_offset']
                payload = get('/api/v1/vendors?' + urllib.parse.urlencode({'limit': mapping.PAGE_SIZE, 'offset': offset}))
                vendor_ids, following, metadata = mapping.page(payload, offset)
                if pending['metadata'] is not None and pending['metadata'] != metadata:
                    obsolete = True
                    mapping.invalid()
                if set(pending['vendor_ids']) & set(vendor_ids):
                    mapping.invalid()
                pending['metadata'] = metadata
                pending['vendor_ids'].extend(vendor_ids)
                pending['next_offset'] = following
                write('pending.json', pending)
            if pending['next_offset'] is None:
                for vendor_id in pending['vendor_ids']:
                    if vendor_id in pending['details']:
                        continue
                    if attempt['requests'] >= budget:
                        break
                    data = mapping.vendor(get('/api/v1/vendors/' + vendor_id), int(vendor_id))
                    pending['details'][vendor_id] = {'data': data}
                    write('pending.json', pending)
        except BudgetExhausted:
            pass
        complete = pending['next_offset'] is None and len(pending['details']) == len(pending['vendor_ids'])
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
        attempt['spends'] = spends + [{'requests': attempt['requests'], 'finished_at': attempt['finished_at']}]
        write('attempt.json', attempt)
        return {'configured': True, 'complete': complete, 'pending': not complete,
                'requests': attempt['requests'], 'total_requests': pending['total_requests']}
    except (Failure, OSError, KeyboardInterrupt) as error:
        attempt['finished_at'] = iso(now())
        attempt['spends'] = spends + [{'requests': attempt['requests'], 'finished_at': attempt['finished_at']}]
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
                   'vendor sightings separate; listed stack prices and quantities; faction from side')]


def index_sources(data):
    rows = [row for item in data['items'].values() for row in item['vendors']]
    attempt = read('attempt.json') or {}
    return provenance(data, rows, age(data['fetched_at']) > 86400 or attempt.get('failed', False))
