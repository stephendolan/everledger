from copy import deepcopy

from everledger_core import Failure

PAGE_SIZE = 100


def invalid():
    raise Failure('ForeverDB API shape mismatch; check vendor mapping before syncing', 4)


def identifier(value):
    if type(value) is not int or value <= 0:
        invalid()
    return value


def text(value):
    if not isinstance(value, str) or not value.strip():
        invalid()
    return value


def basis(record):
    for field in ('seenInForever', 'classicData'):
        if record.get(field) is not None and type(record[field]) is not bool:
            invalid()
    return 'seen' if record.get('seenInForever') is True else 'baseline' if record.get('classicData') is True else 'unverified'


def page(payload, offset):
    if not isinstance(payload, dict) or not isinstance(payload.get('data'), list) or not isinstance(payload.get('meta'), dict):
        invalid()
    meta = payload['meta']
    total = meta.get('total')
    if type(total) is not int or total < 0 or meta.get('limit') != PAGE_SIZE or meta.get('offset') != offset:
        invalid()
    if len(payload['data']) != min(PAGE_SIZE, max(0, total - offset)):
        invalid()
    ids = []
    for row in payload['data']:
        if not isinstance(row, dict):
            invalid()
        key = str(identifier(row.get('id')))
        if key in ids:
            invalid()
        ids.append(key)
    following = offset + PAGE_SIZE if offset + len(ids) < total else None
    return ids, following, {'build': text(meta.get('build')), 'vendor_total': total,
                            'published_at': None}


def vendor(payload, expected_id):
    if not isinstance(payload, dict) or not isinstance(payload.get('data'), dict):
        invalid()
    payload = payload['data']
    if identifier(payload.get('id')) != expected_id or not isinstance(payload.get('sells'), list):
        invalid()
    name = text(payload.get('name'))
    zone = text(payload.get('zone'))
    location = {'zone': zone, 'zone_slug': zone.casefold().replace(' ', '-'),
                'mapId': payload.get('mapId'), 'x': payload.get('x'), 'y': payload.get('y')}
    for axis in ('x', 'y'):
        value = location[axis]
        if value is not None and (type(value) not in (int, float) or not 0 <= value <= 100):
            invalid()
    rows = []
    for sale in payload['sells']:
        if not isinstance(sale, dict):
            invalid()
        item_id = identifier(sale.get('itemId'))
        price, stack = sale.get('price'), identifier(sale.get('stack'))
        if price is not None and (type(price) is not int or price < 0):
            invalid()
        if 'price' not in sale or type(sale.get('limited')) is not bool:
            invalid()
        stock = sale.get('limitedStockCount')
        if stock is not None and (type(stock) is not int or stock < 0):
            invalid()
        claim = basis(sale)
        label = {'seen': 'seen in Forever', 'baseline': 'Classic-only baseline',
                 'unverified': 'unverified'}[claim]
        rows.append({'item': {'id': item_id, 'name': sale.get('name')}, 'row': {
            'npc_id': expected_id, 'name': name, 'title': payload.get('title', ''),
            'zone': zone, 'zone_slug': location['zone_slug'], 'mapId': location['mapId'],
            'side': payload.get('side'),
            'coords': [[location['x'], location['y']]] if location['x'] is not None and location['y'] is not None else [],
            'price_copper': price // stack if price is not None and price % stack == 0 else None,
            'stack_price_copper': price, 'stack': stack,
            'limited': sale['limited'], 'limitedStockCount': stock,
            'status_raw': sale.get('seenInForever'), 'basis': claim,
            'vendor_basis': basis(payload), 'sale_basis': label,
            'raw': {'vendor_seen': payload.get('seenInForever'), 'sell': deepcopy(sale)}}})
    return {'sells': rows}
