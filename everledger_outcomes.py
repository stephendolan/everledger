import base64
import json
import sys

from everledger_core import (Failure, character_path, item_basis, item_matches, money,
                             roster, recipes, sources, stale_code, time_selector)


EVENT_TYPES = ('purchase', 'sale', 'posting', 'expiration', 'cancellation')


def event_params(args):
    types = getattr(args, 'type', None)
    if types is not None:
        types = ','.join(t.strip() for t in types.split(','))
        if any(t not in EVENT_TYPES for t in types.split(',')):
            raise Failure('--type must contain: ' + ', '.join(EVENT_TYPES), 2)
    character = args.char
    if character:
        selected = roster(character_path(character))
        character = selected.get('character', selected)['name']
    params = {'char': character, 'venue': getattr(args, 'venue', None),
              'type': types, 'limit': getattr(args, 'limit', None)}
    after = getattr(args, 'after', None)
    if after is not None:
        if not after:
            raise Failure('empty --after; bootstrap with: everledger events --limit 0 --json', 2)
        if args.since is not None or args.until is not None:
            raise Failure('--after cannot be combined with --since or --until', 2)
        params['after'] = after
    else:
        default = None if args.command == 'events' and args.limit == 0 else ('24h' if args.command == 'events' else '7d')
        params['since'] = time_selector(args.since or default) if args.since or default else None
        params['until'] = time_selector(args.until) if args.until else None
        if params['since'] and params['until'] and params['since'] >= params['until']:
            raise Failure('--since must precede exclusive --until', 2)
    return params


def event_rows(data):
    return data.get('events', data.get('entries', []))


def actor(event):
    return event.get('character') or event.get('actor', {}).get('character')


def resync_params(error, params):
    body = (error.body or {}).get('response') or {}
    if (error.body or {}).get('status') == 409 and body.get('error_code') == 'cursor_invalid' and body.get('resync_since') is not None:
        print('cursor_invalid: resynchronizing once from ' + str(body['resync_since']), file=sys.stderr)
        return {**{k: v for k, v in params.items() if k not in ('after', 'cursor')},
                'since': body['resync_since']}
    return None


VIEW_KEYS = ('char', 'item', 'venue', 'type')
PAGE_PREFIX = 'elpage1:'


def page_cursor(cursor, view, until=None):
    if not view and until is None:
        return cursor
    payload = json.dumps({'cursor': cursor, 'view': view, 'until': until}, separators=(',', ':')).encode()
    return PAGE_PREFIX + base64.urlsafe_b64encode(payload).decode()


def page_params(params):
    params = dict(params)
    cursor = params.get('cursor')
    if cursor and cursor.startswith(PAGE_PREFIX):
        try:
            page = json.loads(base64.b64decode(cursor[len(PAGE_PREFIX):], altchars=b'-_', validate=True))
            if not isinstance(page['cursor'], str) or not page['cursor'] or not isinstance(page['view'], dict):
                raise ValueError()
            if any(k not in VIEW_KEYS or not isinstance(v, (str, dict) if k == 'item' else str)
                   for k, v in page['view'].items()):
                raise ValueError()
            if page.get('until') is not None and not isinstance(page['until'], (str, int)):
                raise ValueError()
            params.update(page['view'], cursor=page['cursor'], until=page.get('until'))
        except (ValueError, KeyError, TypeError):
            raise Failure('invalid snapshot page cursor', 2)
    return params


def in_view(event, view):
    for key in ('type', 'venue'):
        if view.get(key) and event.get(key) not in view[key].split(','):
            return False
    if view.get('char') and (actor(event) or '').casefold() != view['char'].casefold():
        return False
    item = view.get('item')
    if isinstance(item, dict):
        return item_matches(event, item)
    if item:
        recorded = event.get('item') or {}
        return str(recorded.get('id')) == item or (recorded.get('name') or '').casefold() == item.casefold()
    return True


def event_page(params):
    params = page_params(params)
    view = {k: params[k] for k in VIEW_KEYS if params.get(k) is not None}
    wire = {k: v for k, v in params.items() if k not in VIEW_KEYS}
    if params.get('cursor'):
        wire = {k: v for k, v in wire.items() if k not in ('since', 'until')}
    data = dict(roster('/events', wire))
    for key in ('events', 'entries'):
        if key in data:
            data[key] = [e for e in data[key] if in_view(e, view)]
    more = data.get('has_more', False)
    cursor = data.get('next_cursor')
    if more:
        if not cursor:
            raise Failure('events pagination did not advance; refusing a partial tally')
        data['next_cursor'] = page_cursor(cursor, view, params.get('until'))
    else:
        data['next_cursor'] = None
    data['after_cursor'] = None if more else data.get('checkpoint')
    return data


def fetch_events(params):
    bootstrap = params.get('limit') == 0
    if bootstrap:
        data = collect_events(params, discard=True)
    else:
        try:
            return event_page(params)
        except Failure as e:
            replacement = resync_params(e, page_params(params))
            if replacement is None:
                raise
            return event_page(replacement)
    data.pop('events', None)
    data['entries'] = []
    if not data.get('checkpoint') and data.get('available') is not False:
        raise Failure('finished bootstrap has no service checkpoint')
    return data


def collect_events(params, discard=False):
    params = page_params(params)
    for attempt in range(2):
        try:
            data = event_page({**params, 'limit': 500})
            events = [] if discard else list(event_rows(data))
            seen = set()
            while data.get('has_more'):
                cursor = data.get('next_cursor')
                if not cursor or cursor in seen:
                    raise Failure('events pagination did not advance; refusing a partial tally')
                seen.add(cursor)
                data = event_page({'limit': 500, 'cursor': cursor})
                if not discard:
                    events.extend(event_rows(data))
            data = {k: v for k, v in data.items() if k != 'entries'}
            return {**data, 'events': events}
        except Failure as e:
            if attempt:
                raise
            replacement = resync_params(e, params)
            if replacement is not None:
                params = replacement
            elif (e.body or {}).get('status') == 409 and (e.body.get('response') or {}).get('error_code') == 'snapshot_changed':
                print('snapshot_changed: restarting the complete query once', file=sys.stderr)
            else:
                raise


def item_events(args, item, types=None):
    params = event_params(args)
    if types:
        params['type'] = types
    params['item'] = item or getattr(args, 'item', None)
    data = collect_events(params)
    if item:
        data['events'] = [dict(e, match_basis=item_basis(e)) for e in data.get('events', [])]
    return data


def event_lines(events):
    result = []
    for e in events:
        item = e.get('item') or {}
        basis = ' | item matched by name' if e.get('match_basis') == 'name' or item.get('id_basis') == 'name' or item.get('id') is None and item.get('name') else ''
        result.append(f"{actor(e)} | {e.get('recorded_at')} | {e['type']} | {e.get('count')}x {item.get('name') or item.get('id') or ''} | cash {money(e.get('cash_copper'))}{basis}")
    return result or ['none recorded']


def total(rows, field):
    values = [r.get(field) for r in rows]
    return sum(values) if all(v is not None for v in values) else None


def listings(params, prices=None):
    data = roster('/listings', params)
    scan_sources = [scan.get('pricing', {}).get('source') for scan in data.get('scans', []) if scan.get('pricing')]
    data['sources'] = sources(data) + [s for s in scan_sources if s]
    for row in data.get('listings', []):
        item = row.get('item') or {}
        iid = item.get('id', row.get('item_id'))
        quote = (prices or {}).get(iid, {})
        floor = quote.get('floor_copper')
        count = row.get('count')
        recorded = row.get('price', {}).get('recorded') or {}
        for key in ('buyout_copper', 'bid_copper', 'has_bid', 'deposit_copper', 'expires_at'):
            row.setdefault(key, recorded.get(key))
        row.setdefault('available', row.get('price', {}).get('available', False))
        buyout = row.get('buyout_copper')
        available = row['available']
        row['floor_copper'] = floor
        row['undercut'] = (buyout / count > floor) if available and floor is not None and buyout is not None and count else None
    return data


def listing_sources(data):
    scans = [scan['pricing']['source'] for scan in data.get('scans', [])
             if scan.get('pricing') and scan['pricing'].get('source')]
    row_sources = [row['price']['source'] for row in data.get('listings', [])
                   if row.get('price') and row['price'].get('source')]
    return scans or row_sources or sources(data)


def listing_lines(data):
    result = []
    for row in data.get('listings', []):
        item = row.get('item') or {}
        result.append(f"{row.get('character')} | {row.get('count')}x {item.get('name') or row.get('name') or item.get('id') or row.get('item_id')} | buyout {money(row.get('buyout_copper'))} | expires {row.get('expires_at')} | floor {money(row.get('floor_copper'))} | undercut {row.get('undercut')}")
    return result or ['none recorded']


def net(args, item, cat_source, catalogue):
    data = item_events(args, item)
    rows = data.pop('events', [])
    by_type = {t: [e for e in rows if e['type'] == t and e.get('venue') == 'auction']
               for t in ('posting', 'sale', 'expiration', 'cancellation')}
    current = listings({'char': args.char, 'item': str(item['id'])})
    listed_rows = [r for r in current.get('listings', []) if item_matches(r, item)]
    counts = {t: total(es, 'count') if es or t != 'posting' else None for t, es in by_type.items()}
    if data.get('available') is False:
        counts = {t: None for t in counts}
    listing_known = current.get('available', True) and not any(s.get('stale') for s in listing_sources(current))
    if any(not scan.get('available') or not (scan.get('pricing') or {}).get('available', False) for scan in current.get('scans', [])):
        listing_known = False
    if any(r.get('available') is False for r in listed_rows):
        listing_known = False
    listed = total(listed_rows, 'count') if listing_known else None
    values = [counts[t] for t in ('posting', 'sale', 'expiration', 'cancellation')] + [listed]
    unaccounted = values[0] - sum(values[1:]) if all(v is not None for v in values) else None
    cash = total([r for es in by_type.values() for r in es], 'cash_copper')
    proceeds = total(by_type['sale'], 'proceeds_copper')
    if data.get('available') is False:
        cash = proceeds = None
    deposits_lost = None
    materials = [r['reagents'] for _, r in recipes(catalogue) if r['result'].get('item_id') == item['id']]
    summary = {**data, 'item': item, 'posted': counts['posting'], 'sold': counts['sale'], 'expired': counts['expiration'],
               'cancelled': counts['cancellation'], 'listed': listed, 'unaccounted': unaccounted,
               'cash_copper': cash, 'proceeds_copper': proceeds, 'deposits_lost_copper': deposits_lost,
               'materials': materials, 'materials_copper': None, 'realized_profit_copper': None,
               'basis': 'unknown: no supported lot assignments', 'reconciliation': 'unverified: real posting/invoice/expiry cycle required',
               'match_basis': sorted({item_basis(e) for e in rows}),
               'sources': sources(data) + sources(current) + [cat_source]}
    lines = [item.get('name') or str(item['id']),
             'posted unknown' if counts['posting'] is None else f"posted {counts['posting']} (posting attempts)",
             f"sold {counts['sale']} | expired {counts['expiration']} | cancelled {counts['cancellation']} | listed {listed}",
             f'unaccounted {unaccounted} (signed diagnostic: posted - sold - expired - cancelled - listed)',
             f'proceeds {money(proceeds)} | auction cash {money(cash)} before materials',
             'realized profit unknown; holding/materials basis unknown; real posting cycle unverified']
    if unaccounted == 0:
        lines.append('deposits lost ' + money(deposits_lost))
    if data.get('available') is False:
        lines.append(f"journal {data.get('state')}: {data.get('reason')}")
    if 'name' in summary['match_basis']:
        lines.append('item match basis: name (linkless journal rows)')
    return summary, lines


def positions(args, item, prices, market_source):
    chars = roster('/characters')
    entries = chars.get('characters', [])
    if args.char:
        selected = roster(character_path(args.char))
        entries = [selected.get('character', selected)]
    journal = item_events(args, item, 'purchase')
    current = listings({'char': args.char, 'item': str(item['id']) if item else args.item})
    rows = {}
    used_sources = sources(chars) + sources(journal) + sources(current) + [market_source]
    freshness_sources = sources(chars) + listing_sources(current) + [market_source]
    availability = []
    for ch in entries:
        name = ch.get('name') or ch.get('short_name')
        inv = roster(character_path(name) + '/inventory')
        used_sources.extend(sources(inv))
        freshness_sources.extend(sources(inv))
        for location in ('bags', 'bank', 'equipped', 'mail'):
            section = inv.get(location, {})
            availability.append({'character': name, 'location': location, 'available': section.get('available', False)})
            for r in section.get('items', []):
                if item and not item_matches(r, item):
                    continue
                iid = r['item_id']
                row = rows.setdefault(iid, {'item': {'id': iid, 'name': r.get('name')}, 'held': 0, 'locations': [], 'listed': 0})
                row['held'] += r['count']
                row['locations'].append({'character': name, 'location': location, 'count': r['count']})
    for r in current.get('listings', []):
        ri = r.get('item') or {'id': r.get('item_id'), 'name': r.get('name')}
        if item and not item_matches(r, item):
            continue
        row = rows.setdefault(ri['id'], {'item': ri, 'held': 0, 'locations': [], 'listed': 0})
        row['listed'] += r.get('count') or 0
    lines = []
    for iid, row in rows.items():
        floor = prices.get(iid, {}).get('floor_copper')
        purchases = [e for e in journal.get('events', []) if item_matches(e, row['item'])]
        row.update(floor_copper=floor, value_copper=floor * row['held'] if floor is not None else None,
                   basis_copper=None, basis='unknown: no supported lot assignments',
                   purchases=purchases,
                   match_basis=sorted({item_basis(e) for e in purchases}))
        lines.append(f"{row['item']['name'] or iid} | recorded held {row['held']} | listed {row['listed']} | basis unknown | floor {money(floor)} | held value {money(row['value_copper'])}")
    if any(not a['available'] for a in availability):
        lines.append('holdings incomplete: some inventory locations are unavailable')
    return {'positions': list(rows.values()), 'inventory_availability': availability,
            'complete': all(a['available'] for a in availability), 'stale': stale_code(freshness_sources) == 3,
            'journal_available': journal.get('available'), 'journal_state': journal.get('state'),
            'sources': used_sources}, lines or ['none recorded']
