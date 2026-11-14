"""Provisional ForeverDB wire mapping. Confirm every assumption on key day.

PROVISIONAL: list envelope is {vendors: [{id, updated_at}], next_page,
build, extracted_at}; page/per_page are numeric, terminal next_page is null.
updated_at is a nonempty revision covering locations, labels AND the full sells
list (including removals/prices). build/extracted_at describe one consistent
extraction across pages. No seen filter: Classic-only records must survive.

PROVISIONAL: detail is an unwrapped {id, name, title?, seen?, locations, sells}.
Locations are [{zone, zone_slug, x, y}] with percentage coordinates or nulls.
Sells are [{item_id, name?, price_copper, seen?}]; prices are integer copper or
null, with no purchase quantity inference. IDs are positive JSON integers.
PROVISIONAL: boolean seen true means seen-in-Forever, false means Classic-only;
missing/null is unverified. Vendor and sell labels are independent; a vendor
sighting never promotes its sales or prices. There is no faction inference.
"""
from copy import deepcopy

from everledger_core import Failure


def invalid():
    raise Failure('ForeverDB provisional API shape mismatch; confirm mapping before syncing', 4)


def identifier(value):
    if type(value) is not int or value <= 0:
        invalid()
    return value


def text(value):
    if not isinstance(value, str) or not value.strip():
        invalid()
    return value


def basis(record):
    value = record.get('seen')
    if value is not None and type(value) is not bool:
        invalid()
    return 'unverified' if value is None else 'seen' if value else 'baseline'


def page(payload, number):
    if not isinstance(payload, dict) or not isinstance(payload.get('vendors'), list):
        invalid()
    if 'next_page' not in payload:
        invalid()
    following = payload['next_page']
    if following is not None and (type(following) is not int or following != number + 1):
        invalid()
    revisions = {}
    for vendor in payload['vendors']:
        if not isinstance(vendor, dict):
            invalid()
        key = str(identifier(vendor.get('id')))
        if key in revisions:
            invalid()
        revisions[key] = text(vendor.get('updated_at'))
    return revisions, following, {'build': text(payload.get('build')),
                                 'published_at': text(payload.get('extracted_at'))}


def vendor(payload, expected_id):
    if not isinstance(payload, dict) or identifier(payload.get('id')) != expected_id:
        invalid()
    name = text(payload.get('name'))
    if not isinstance(payload.get('locations'), list) or not isinstance(payload.get('sells'), list):
        invalid()
    locations = []
    for location in payload['locations']:
        if not isinstance(location, dict):
            invalid()
        text(location.get('zone'))
        text(location.get('zone_slug'))
        for axis in ('x', 'y'):
            value = location.get(axis)
            if value is not None and (type(value) not in (int, float) or not 0 <= value <= 100):
                invalid()
        locations.append(deepcopy(location))
    first = locations[0] if locations else {'zone': 'Unknown', 'zone_slug': 'unknown'}
    rows = []
    for sale in payload['sells']:
        if not isinstance(sale, dict):
            invalid()
        item_id = identifier(sale.get('item_id'))
        price = sale.get('price_copper')
        if 'price_copper' not in sale or (price is not None and (type(price) is not int or price < 0)):
            invalid()
        claim = basis(sale)
        label = {'seen': 'seen in Forever', 'baseline': 'Classic-only baseline',
                 'unverified': 'unverified'}[claim]
        rows.append({'item': {'id': item_id, 'name': sale.get('name')}, 'row': {
            'npc_id': expected_id, 'name': name, 'title': payload.get('title', ''),
            'zone': first['zone'], 'zone_slug': first['zone_slug'],
            'locations': deepcopy(locations),
            'coords': [[p['x'], p['y']] for p in locations if p.get('x') is not None and p.get('y') is not None],
            'price_copper': price, 'price_raw': price,
            'status_raw': sale.get('seen'), 'status': claim, 'basis': claim,
            'vendor_basis': basis(payload), 'sale_basis': label, 'price_basis': label,
            'raw': {'vendor_seen': payload.get('seen'), 'sell': deepcopy(sale)}}})
    return {'sells': rows}
