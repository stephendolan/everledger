#!/usr/bin/env python3
"""Differential fuzz: random well-formed fixture worlds, oracle against candidate.

Usage: python3 parity/fuzz.py CANDIDATE [--suite read|sync] [--cases N] [--seed S] [--commands a,b,c]

The replayed corpus is fixed, so an implementation can pass it by fitting it.
Each case here builds a random world inside the shapes the oracle's contract
allows (nulls where the oracle tolerates null, absent keys where it reads with
a default, every provenance spelling), then runs random invocations through
the same engine and comparison policy as the replayed corpus. A difference is
a finding; the case and step names reproduce it with --seed and --only.
"""
import argparse
import datetime
import json
import os
import random
import sys
import tempfile
import types
import urllib.parse
from pathlib import Path

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.engine import Engine, ORIGIN, ROOT, jsonable
from parity.fixture import Fixture

MARKET = 'forever.pvp.alliance.us'
T0 = datetime.datetime.fromisoformat(ORIGIN.replace('Z', '+00:00'))
ZONES = ['UTC', 'America/New_York', 'Asia/Kolkata', 'Europe/London']
PROFESSIONS = ['tailoring', 'leatherworking', 'alchemy']
SOURCE_TYPES = ['Trainer', 'Pattern · Drop', 'Pattern · Vendor', 'Automatic', 'Not yet known', 'Quest', None]
NAMES = ['Red Dye', 'Bolt of Woolen Cloth', "White Swashbuckler's Shirt", 'Light Leather', 'Élixir of Fortitude',
         'Straße Boots', 'Heavy Hide', 'Coarse Thread', 'Silk Cloth', 'Mageweave', 'Rugged Leather', 'Fine Thread',
         'Wild Steelbloom', 'Briarthorn', 'Empty Vial', 'Bright Baubles', 'Robes of Arcana', 'Dark Silk Shirt']
CHARACTERS = ['Slade Undertow', 'Mira', 'Ötzi Frost', "Kael'thas Sunstrider", 'Bank Alt']
EVENT_TYPES = ['purchase', 'sale', 'posting', 'expiration', 'cancellation']


def moment(rng, low, high, style=None):
    """An ISO timestamp between low and high seconds before the origin, in one of the oracle's spellings."""
    when = T0 - datetime.timedelta(seconds=rng.uniform(low, high))
    style = style or rng.choice(['z', 'z', 'micro', 'offset', 'local-offset'])
    if style == 'z':
        return when.isoformat(timespec='seconds').replace('+00:00', 'Z')
    if style == 'micro':
        return when.isoformat().replace('+00:00', 'Z')
    if style == 'offset':
        return when.isoformat(timespec='seconds')
    return when.astimezone(datetime.timezone(datetime.timedelta(hours=-5))).isoformat(timespec='seconds')


def provenance(rng, name=None):
    """One source entry: every key the footer reads is optional, and ages may be int, float, or null."""
    stale = rng.random() < 0.25
    age = rng.choice([None, rng.randrange(0, 5000), round(rng.uniform(0, 5000), 1)])
    entry = {}
    label = name or rng.choice(['Journalator.lua', 'DataStore_Auctions.lua', 'DataStore_Crafts.lua', 'Other.lua'])
    kind = rng.choice(['file', 'file', 'path', 'name'])
    entry[kind] = rng.choice(['', '/saved/', '/Users/x/SavedVariables/']) + label if kind != 'name' else label
    when_key = rng.choice(['mtime', 'saved_at', 'scanned_at', 'updated_at', None])
    if when_key:
        entry[when_key] = rng.choice([moment(rng, 0, 900000), None])
    age_key = rng.choice(['file_age_seconds', 'age_seconds', 'scan_age_seconds', None])
    if age_key:
        entry[age_key] = age
    if rng.random() < 0.7:
        entry['stale'] = stale
    if rng.random() < 0.15:
        entry['detail'] = rng.choice(['Data from Elsewhere (https://example.invalid); extra', 'plain detail', ''])
    return entry


def provenance_list(rng):
    rows = [provenance(rng) for _ in range(rng.randrange(0, 4))]
    if rows and rng.random() < 0.15:
        return {f'source{i}': row for i, row in enumerate(rows)}
    return rows


def catalogue(rng, items):
    spell = [rng.randrange(1000, 1100)]

    def recipe():
        spell[0] += 1
        learn = rng.randrange(1, 250)
        shape = rng.choice(['full', 'full', 'full', 'orange', 'grey-only'])
        if shape == 'full':
            yellow = learn + rng.randrange(1, 30)
            green = yellow + rng.randrange(1, 30)
            bands = {'learn': learn, 'yellow': yellow, 'green': green, 'grey': green + rng.randrange(1, 30)}
        elif shape == 'orange':
            bands = {'learn': learn, 'yellow': None, 'green': None, 'grey': None}
        else:
            bands = {'learn': learn, 'yellow': None, 'green': None, 'grey': learn + rng.randrange(5, 60)}
        result = rng.choice(items)
        linked = rng.random() < 0.9
        return {'spell_id': spell[0], 'name': rng.choice([result[1], 'Craft ' + result[1], result[1] + ' II']),
                'bands': bands,
                'result': {'item_id': result[0] if linked else None, 'name': result[1], 'qty': rng.choice([1, 1, 1, 2, 5])},
                'reagents': [{'item_id': r[0], 'name': r[1], 'qty': rng.randrange(1, 9)}
                             for r in rng.sample(items, rng.randrange(0, 4))],
                'source_type': rng.choice(SOURCE_TYPES)}

    professions = {}
    for name in rng.sample(PROFESSIONS, rng.randrange(1, len(PROFESSIONS) + 1)):
        rows = [recipe() for _ in range(rng.randrange(0, 9))]
        professions[name] = {'stated_count': len(rows) + rng.choice([0, 0, 0, 1]), 'recipes': rows}
    generated = moment(rng, 0, 86400, 'z') if rng.random() < 0.8 else moment(rng, 15 * 86400, 40 * 86400, 'z')
    return {'generated_at': generated, 'source': 'wowforever.ninja profession index pages (Forever client data)',
            'professions': professions}


def character(rng, name, cat):
    professions = []
    for prof in rng.sample(list(cat['professions']), rng.randrange(0, len(cat['professions']) + 1)):
        recipes = cat['professions'][prof]['recipes']
        entry = {'name': prof.capitalize(), 'skill_available': rng.random() < 0.8}
        if entry['skill_available']:
            entry['skill'] = rng.randrange(1, 300)
            entry['max_skill'] = rng.choice([75, 150, 225, 300, None])
        else:
            entry['reason'] = rng.choice(['profession window never opened', None])
            if rng.random() < 0.5:
                entry['skill'] = None
        kind = rng.choice(['known', 'known', 'empty', 'unavailable', 'absent', 'null-known'])
        if kind == 'known':
            chosen = rng.sample(recipes, rng.randrange(0, len(recipes) + 1))
            entry['recipes'] = {'available': True, 'known': [{'spell_id': r['spell_id']} for r in chosen]
                                + [{'spell_id': 999000 + i} for i in range(rng.choice([0, 0, 2]))]}
        elif kind == 'empty':
            entry['recipes'] = {'available': True, 'known': []}
        elif kind == 'unavailable':
            entry['recipes'] = {'available': False, 'known': None}
        elif kind == 'null-known':
            entry['recipes'] = {'available': True, 'known': None}
        professions.append(entry)
    row = {'name': name, 'level': rng.choice([None, 1, 20, 60]), 'race': rng.choice(['Human', 'Gnome', None]),
           'class': rng.choice(['Mage', 'Rogue', None]), 'faction': rng.choice(['Alliance', None]),
           'realm': rng.choice(['Forever', '']), 'gold_copper': rng.choice([None, 0, 5, 123456, 10000, 99]),
           'hearth': rng.choice([None, {'location': 'Ironforge'}, {'location': None}, {}]),
           'professions_updated_at': rng.choice([None, moment(rng, 0, 90000)]), 'professions': professions}
    if rng.random() < 0.5:
        row['short_name'] = name.split()[0]
    for key in rng.sample(['level', 'race', 'class', 'faction', 'realm', 'gold_copper', 'hearth'], rng.randrange(0, 3)):
        del row[key]
    return row


def inventory(rng, items):
    result = {}
    for location in rng.sample(['bags', 'bank', 'equipped', 'mail'], rng.randrange(0, 5)):
        rows = [{'item_id': item[0], 'name': rng.choice([item[1], item[1], None]), 'count': rng.randrange(1, 40)}
                for item in rng.sample(items, rng.randrange(0, 4))]
        section = {'items': rows}
        if rng.random() < 0.85:
            section['available'] = rng.random() < 0.8
        result[location] = section
    if rng.random() < 0.8:
        result['sources'] = provenance_list(rng)
    return result


def listings(rng, items, names):
    def scan_source():
        return {'file': 'DataStore_Auctions.lua', 'scanned_at': moment(rng, 0, 3000), 'stale': rng.random() < 0.2}

    rows = []
    for _ in range(rng.randrange(0, 6)):
        item = rng.choice(items)
        row = {'character': rng.choice(names + [None]), 'count': rng.choice([1, 2, 5, 20, None, 0])}
        if rng.random() < 0.5:
            row['item'] = {'id': item[0], 'name': rng.choice([item[1], None])}
        else:
            row['item_id'] = item[0]
            row['name'] = rng.choice([item[1], None])
        buyout = rng.choice([rng.randrange(1, 90000), None])
        shape = rng.choice(['flat', 'nested', 'nested', 'bare'])
        if shape == 'flat':
            row['buyout_copper'] = buyout
            if rng.random() < 0.6:
                row['available'] = rng.random() < 0.8
            if rng.random() < 0.5:
                row['expires_at'] = moment(rng, -90000, 0)
        elif shape == 'nested':
            recorded = {'buyout_copper': buyout, 'bid_copper': rng.choice([None, 10]), 'has_bid': rng.random() < 0.3,
                        'deposit_copper': rng.choice([None, 25]), 'expires_at': rng.choice([None, moment(rng, -90000, 0)])}
            for key in rng.sample(list(recorded), rng.randrange(0, 3)):
                del recorded[key]
            price = {'recorded': rng.choice([recorded, recorded, None])}
            if rng.random() < 0.85:
                price['available'] = rng.random() < 0.8
            if rng.random() < 0.5:
                price['source'] = scan_source()
            row['price'] = price
        rows.append(row)
    data = {'listings': rows}
    if rng.random() < 0.7:
        scans = []
        for _ in range(rng.randrange(0, 3)):
            scan = {'available': rng.random() < 0.85}
            kind = rng.choice(['priced', 'priced', 'unpriced', 'none'])
            if kind == 'priced':
                scan['pricing'] = {'available': rng.random() < 0.85, 'source': scan_source()}
            elif kind == 'unpriced':
                scan['pricing'] = {'available': False}
            scans.append(scan)
        data['scans'] = scans
    if rng.random() < 0.8:
        data['sources'] = provenance_list(rng)
    if rng.random() < 0.4:
        data['counts'] = {'matched': 99, 'returned': 99}
    if rng.random() < 0.2:
        data.update(matched=7, returned=7)
    if rng.random() < 0.1:
        data['available'] = False
    return data


def events(rng, items, names):
    rows = []
    for index in range(rng.randrange(0, 14)):
        item = rng.choice(items)
        kind = rng.choice(EVENT_TYPES)
        row = {'id': f'e{index}', 'type': kind, 'venue': rng.choice(['auction', 'auction', 'vendor']),
               'count': rng.choice([1, 2, 3, 10, None]), 'cash_copper': rng.choice([None, -500, -13, 0, 250, 38400]),
               'recorded_at': rng.choice([None, moment(rng, 0, 600000)])}
        who = rng.choice(names + [None])
        if rng.random() < 0.8:
            row['character'] = who
        else:
            row['actor'] = {'character': who}
        shape = rng.choice(['linked', 'linked', 'linkless', 'name-basis', 'null', 'absent'])
        if shape == 'linked':
            row['item'] = {'id': item[0], 'name': item[1]}
        elif shape == 'linkless':
            row['item'] = {'id': None, 'name': rng.choice([item[1], item[1].upper(), item[1].lower()])}
        elif shape == 'name-basis':
            row['item'] = {'id': item[0], 'name': item[1], 'id_basis': 'name'}
        elif shape == 'null':
            row['item'] = None
        if kind == 'sale':
            row['proceeds_copper'] = rng.choice([None, 95, 38000])
            row['value_copper'] = 40000
        rows.append(row)
    page_size = rng.choice([2, 3, 5, 500])
    key = rng.choice(['entries', 'events'])
    absent = rng.random() < 0.08
    # A snapshot breaks once at its second page; one started from the resync
    # hint does not. The response depends on the query alone, so the oracle's
    # run and the candidate's run see the same service.
    broken = rng.choice([None, None, page_size])
    hint, hint_epoch = rng.choice([(1794614400, '1794614400'), ('1794614400', '1794614400'),
                                   ('2026-11-14T00:00:00Z', '1794614400'), (0, '0')])
    page_sources = [provenance_list(rng) for _ in range(4)]
    extras = rng.random() < 0.4

    def respond(query):
        for bound in ('since', 'until'):
            if bound in query and not query[bound][0].isdigit():
                return 400, {'error_code': 'invalid_query', 'message': 'Expected bounded decimal integer'}
        limit = int(query.get('limit', ['500'])[0])
        if not 1 <= limit <= 500:
            return 400, {'error_code': 'invalid_query', 'message': 'Integer outside allowed bounds'}
        if absent:
            return 200, {'available': False, 'state': 'absent', 'reason': 'Journalator.lua has not been saved',
                         key: [], 'sources': [{'file': 'Journalator.lua', 'mtime': None}],
                         'next_cursor': None, 'has_more': False}
        family = 'resynced' if query.get('since', [''])[0] == hint_epoch else 'page'
        cursor = query.get('cursor', [family + ':0'])[0]
        family, _, position = cursor.partition(':')
        if family not in ('page', 'resynced') or not position.isdigit():
            return 409, {'error_code': 'cursor_invalid', 'message': 'unknown cursor'}
        offset = int(position)
        if 'cursor' in query and family == 'page' and offset == broken:
            return 409, {'error_code': 'cursor_invalid', 'resync_since': hint}
        if 'after' in query:
            after = query['after'][0]
            if not (after.startswith('jnrc1:') and after[6:].isdigit()):
                return 409, {'error_code': 'cursor_invalid', 'resync_since': hint}
            offset = int(after[6:])
        page = rows[offset:offset + min(limit, page_size)]
        more = offset + len(page) < len(rows)
        body = {key: page, 'has_more': more, 'next_cursor': f'{family}:{offset + len(page)}' if more else None,
                'checkpoint': None if more else f'jnrc1:{len(rows)}',
                'sources': page_sources[(offset // max(1, page_size)) % len(page_sources)]}
        if extras:
            body.update(mode='incremental' if 'after' in query else 'snapshot',
                        counts={'matched': len(rows), 'returned': len(page)}, matched=len(rows), returned=len(page),
                        filters={'since': int(query.get('since', ['0'])[0])})
        return 200, body

    return respond, rows


def pricetable(rng, items, fresh):
    when = T0 - datetime.timedelta(seconds=rng.uniform(0, 600) if fresh else rng.uniform(1000, 500000))
    lines = [f'market|version|{int(when.timestamp())}|2']
    for item in items:
        if rng.random() < 0.8:
            median = rng.choice(['', str(rng.randrange(1, 90000)), f'{rng.randrange(1, 9000)}.5', f'{rng.uniform(1, 9000):.2f}'])
            floor = rng.choice(['', str(rng.randrange(1, 90000)), f'{rng.randrange(1, 9000)}.5'])
            quantity = rng.choice(['', str(rng.randrange(0, 5000))])
            sales = rng.choice(['', str(rng.randrange(0, 9000))])
            lines.append(f'{item[0]}:{median}:{floor}:{quantity}:::::{sales}')
    return '\n'.join(lines)


def quote(rng, item, fresh):
    number = lambda: rng.choice([None, rng.randrange(1, 90000), rng.randrange(1, 9000) + 0.5, round(rng.uniform(1, 9000), 2)])
    observed = moment(rng, 0, 600, 'z') if fresh else moment(rng, 1000, 500000)
    return {'item': {'id': item[0], 'name': item[1]},
            'latest': {'observedAt': rng.choice([observed, observed, observed, None]), 'median': number(),
                       'minBuyout': number(), 'median7d': number(),
                       'quantity': rng.choice([None, 0, 709, 12.0]), 'auctions': rng.choice([None, 1, 13])},
            'sales': {'estimatedPerDay': rng.choice([None, 0, 380.8, 12, 1e-05]),
                      'coveredHours': rng.choice([None, 79.7, 24, 168.0])}}


def pins(rng, item):
    zones = {}
    npc = [rng.randrange(1000, 2000)]
    for slug, name in rng.sample([('ironforge', 'Ironforge'), ('stormwind-city', 'Stormwind City'), ('orgrimmar', 'Orgrimmar'),
                                  ('thunder-bluff', 'Thunder Bluff'), ('darnassus', 'Darnassus'), ('the-barrens', 'The Barrens')],
                                 rng.randrange(0, 5)):
        vendors, placed, codes = {}, [], []
        for _ in range(rng.randrange(0, 5)):
            npc[0] += 1
            title = rng.choice(['Tailoring Supplies', 'Trade Supplier', 'General Goods', ''])
            price = rng.choice(['50c', '1g 2s 3c', '5s', '12g', 'nonsense', '1g 1g', '0c', ' 7s 5c '])
            text = rng.choice([f'{title} · {price}', f'{title} · {price}', title, f' · {price}'])
            vendor = {'n': rng.choice(['Poranna Snowbraid', 'Outfitter Eric', 'Felika', 'Élise', 'burbik gearspanner']) + f' {npc[0]}'}
            if rng.random() < 0.9:
                vendor['s'] = text
            if rng.random() < 0.6:
                vendor['f'] = rng.choice(['u', 'c', 'n', 'r', None])
            vendors[str(npc[0])] = vendor
            for _ in range(rng.choice([0, 1, 1, 1, 3])):
                placed.append([rng.choice([42.9, 43, 0, 99.9, round(rng.uniform(0, 100), 1)]),
                               rng.choice([28.3, 29, round(rng.uniform(0, 100), 1)]), npc[0]])
                codes.append(rng.choice(['u', 'c', 'n', 'r', None]))
        zone = {'vendors': vendors}
        if rng.random() < 0.9:
            zone['zone'] = {'name': name}
        if rng.random() < 0.9:
            zone['pins'] = {'vendor': placed}
        if rng.random() < 0.7:
            zone['codes'] = {'vendor': codes[:rng.randrange(0, len(codes) + 1)]}
        zones[slug] = zone
    return {'item': item[0], 'zones': zones}


def world(rng):
    """Routes, catalogue, and the vocabulary of names and items for one case."""
    ids = rng.sample(range(100, 30000), 9)
    items = list(zip(ids, rng.sample(NAMES, 9)))
    if rng.random() < 0.2:
        items.append((rng.randrange(40000, 41000), items[0][1].upper()))
    names = rng.sample(CHARACTERS, rng.randrange(1, 4))
    cat = catalogue(rng, items)
    routes = {'/': (200, '')}
    roster = [character(rng, name, cat) for name in names]
    routes['/characters'] = (200, {'characters': roster, 'sources': provenance_list(rng)})
    for row in roster:
        body = {'character': row, 'sources': provenance_list(rng)}
        if rng.random() < 0.7:
            body['generated_at'] = rng.choice([None, moment(rng, 0, 9000)])
        if rng.random() < 0.15:
            body = dict(row, sources=provenance_list(rng))
        for selector in {row['name'], row['name'].split()[0]}:
            path = '/characters/' + urllib.parse.quote(selector, safe='')
            routes[path] = (200, body)
            if rng.random() < 0.97:
                routes[path + '/inventory'] = (200, inventory(rng, items))
    routes['/listings'] = (200, listings(rng, items, names))
    fresh = rng.random() < 0.7
    if rng.random() < 0.95:
        routes[f'/v1/pricetable/{MARKET}'] = (200, pricetable(rng, items, fresh))
    for item in items:
        if rng.random() < 0.93:
            routes[f'/v1/prices/{MARKET}/{item[0]}'] = (200, quote(rng, item, fresh))
        kind = rng.choice(['pins', 'pins', 'pins', 'no-pins', 'no-item', 'down'])
        if kind in ('pins', 'no-pins'):
            routes[f'/items/{item[0]}.json'] = (200, {'id': item[0], 'name': rng.choice([item[1], None])})
        if kind == 'pins':
            routes[f'/items/{item[0]}.pins.json'] = (200, pins(rng, item))
        if kind == 'down':
            routes[f'/items/{item[0]}.json'] = (503, {})
    contract, _ = events(rng, items, names)
    return routes, contract, cat, items, names


def selector(rng):
    day = lambda n: (T0 - datetime.timedelta(days=n)).strftime('%Y-%m-%d')
    return rng.choice(['7d', '24h', '3d', '30d', '1h', day(1), day(3), day(40), day(-1),
                       moment(rng, 0, 500000, 'z'), (T0 - datetime.timedelta(days=2)).strftime('%Y-%m-%dT%H:%M:%S'),
                       (T0 - datetime.timedelta(days=2)).strftime('%Y-%m-%dT%H:%M'), 'bad', '2026-13-01', '2099-01-01'])


def invocation(rng, items, names, cat, cursors):
    """One random argv, biased toward valid selectors with an occasional wrong one."""
    item = rng.choice(items)
    known = [entry for block in cat['professions'].values() for r in block['recipes']
             for entry in [r['result'], *r['reagents']] if entry['item_id']]
    named = rng.choice(known)['name'] if known else item[1]
    wanted = rng.choice([str(item[0])] * 3 + [named] * 3 + [named.lower(), item[1], '999999', 'No Such Item'])
    who = rng.choice(names * 3 + [n.split()[0] for n in names] * 2 + ['Nobody'])
    prof = rng.choice(list(cat['professions']) + ['mining', 'Tailoring'])
    window = lambda: [flag for pair in ((('--since', selector(rng)) if rng.random() < 0.4 else ()),
                                        (('--until', selector(rng)) if rng.random() < 0.3 else ())) for flag in pair]
    pick = lambda *flags: [part for flag in flags if rng.random() < 0.35 for part in flag]
    command = rng.choice(['price', 'price', 'vendors', 'vendors', 'recipe', 'bridge', 'margins', 'margins', 'char', 'char',
                          'listings', 'listings', 'positions', 'positions', 'sales', 'purchases', 'expirations',
                          'net', 'net', 'events', 'events', 'events', 'events', 'verify'])
    if command == 'price':
        return ['price', *wanted.split(' ')] if rng.random() < 0.5 else ['price', wanted]
    if command == 'vendors':
        return ['vendors', wanted, *pick(('--in', rng.choice(['IF', 'SW', 'org', 'iron', 'Nowhere', 'thunder', 'barrens', 'TB'])), ('--all',))]
    if command == 'recipe':
        return ['recipe', rng.choice([named, named[:4], named.upper(), 'Craft', 'zzz', 'II']), *pick(('--skill', str(rng.randrange(1, 300))))]
    if command == 'bridge':
        start = rng.randrange(1, 250)
        return ['bridge', prof, str(start), str(start + rng.choice([-5, 0, 10, 60, 150]))]
    if command == 'margins':
        return ['margins', *([prof] if rng.random() < 0.6 else []),
                *pick(('--level', str(rng.randrange(1, 300))), ('--char', who), ('--known', who))]
    if command == 'char':
        if rng.random() < 0.25:
            return ['char']
        return ['char', who, *([rng.choice([prof, prof, 'auctions'])] if rng.random() < 0.6 else []),
                *pick(('--skill', str(rng.randrange(1, 300))))]
    if command in ('listings', 'positions'):
        return [command, *pick(('--char', who), ('--item', wanted))]
    if command in ('sales', 'purchases', 'expirations'):
        venue = pick(('--venue', rng.choice(['auction', 'vendor']))) if command != 'expirations' else []
        return [command, *pick(('--item', wanted), ('--char', who)), *window(), *venue]
    if command == 'net':
        return ['net', wanted, *pick(('--char', who)), *window()]
    if command == 'verify':
        return ['verify']
    types_ = ','.join(rng.sample(EVENT_TYPES, rng.randrange(1, 4)))
    mode = rng.choice(['window', 'window', 'bootstrap', 'after', 'cursor', 'cursor'])
    if mode == 'bootstrap':
        return ['events', '--limit', '0', *pick(('--type', types_), ('--char', who))]
    if mode == 'after':
        token = rng.choice(cursors['after'] + ['jnrc1:0', 'garbage', ''])
        return ['events', '--after', token, *pick(('--type', types_), ('--venue', 'auction'), ('--item', wanted), ('--char', who), ('--limit', str(rng.choice([1, 2, 50]))))]
    if mode == 'cursor' and cursors['page']:
        return ['events', '--cursor', rng.choice(cursors['page']), *pick(('--limit', str(rng.choice([0, 1, 2, 500]))), ('--type', types_))]
    return ['events', *pick(('--type', rng.choice([types_, types_, 'sale, purchase', 'bogus'])), ('--venue', rng.choice(['auction', 'vendor'])),
                            ('--item', wanted), ('--char', who), ('--limit', str(rng.choice([1, 2, 3, 500, 600])))), *window()]


CATALOGUE_PAGES = ['alchemy', 'blacksmithing', 'enchanting', 'engineering', 'leatherworking', 'tailoring',
                   'cooking', 'first-aid', 'fishing', 'mining', 'herbalism', 'skinning']
MARKUP_NAMES = ['Red Shirt', 'White Swashbuckler&#39;s Shirt', 'Fish &amp; Chips', '&quot;Lucky&quot; Charm', 'Élixir of Fortitude',
                'Bolt&nbsp;of Cloth', 'Rune &lt;Major&gt;', 'Caf&eacute; Robe', 'Tr&#xE8;s Belt', 'Plain &amp;amp; Simple']


def profession_page(rng, prof):
    """One profession index page in the markup build_catalogue.py reads, with the rows it skips."""
    rows, recipes = [], 0
    for index in range(rng.choice([0, 0, 1, 3, 6])):
        learn = rng.randrange(1, 300)
        bands = f'<span class="o">{learn}</span>'
        for css in rng.choice([[], ['y'], ['y', 'g'], ['y', 'g', 'x']]):
            learn += rng.randrange(1, 30)
            bands += rng.choice(['', ' ', '\n  ']) + f'<span class="{css}">{learn}</span>'
        name = rng.choice(MARKUP_NAMES)
        link = f'<a href="/professions/{prof}/{rng.randrange(1, 99999)}-slug/" class="r">{name}</a>'
        item = lambda: (f'<a href="/items/{rng.randrange(1, 99999)}-item-slug/" class="q{rng.randrange(0, 5)}">'
                        + rng.choice(['', '<img src="/i.png" alt="">', '<img src="/i.png" alt=""> '])
                        + rng.choice(MARKUP_NAMES) + '</a>' + rng.choice(['', '', ' × 2', '×5', ' ×  12']))
        result = rng.choice([item(), item(), item(), rng.choice(MARKUP_NAMES), '<i>Enchant Bracer</i>'])
        reagents = rng.choice([', ', '<br>', ' ']).join(item() for _ in range(rng.randrange(0, 4)))
        source = rng.choice(['Trainer', 'Pattern &middot; Drop', 'Pattern · Vendor', '', ' Quest ', '<b>Automatic</b>'])
        kind = rng.choice(['recipe'] * 6 + ['no-source-cell', 'no-bands', 'short', 'trainer-row'])
        cell = lambda body: rng.choice(['<td role="cell">', '<td class="c" role="cell">', '<td role="cell" data-x="1">']) + body + '</td>'
        opening = rng.choice(['<tr role="row">', '<tr role="row" class="odd">'])
        if kind == 'recipe':
            rows.append(opening + cell(bands) + cell(link) + cell(result) + cell(reagents) + cell(source) + '</tr>')
            recipes += 1
        elif kind == 'no-source-cell':
            rows.append(opening + cell(bands) + cell(link) + cell(result) + cell(reagents) + '</tr>')
            recipes += 1
        elif kind == 'no-bands':
            rows.append(opening + cell('') + cell(link) + cell(result) + cell(reagents) + cell(source) + '</tr>')
        elif kind == 'short':
            rows.append(opening + cell(bands) + cell(link) + cell(result) + '</tr>')
        else:
            rows.append(opening + cell('Trainer Bob') + cell('Stormwind') + '</tr>')
    stated = recipes + rng.choice([0, 0, 0, 1])
    return f'<html><body><h2>Recipes ({stated})</h2>\n<table>' + '\n'.join(rows) + '</table></body></html>'


def vendor_dataset(rng):
    """ForeverDB list pages and details inside the mapping's accepted shapes."""
    count = rng.choice([0, 1, 3, 5, 8, 30, 40, 45, 120])
    ids = rng.sample(range(1, 90000), count)
    item_ids = rng.sample(range(100, 30000), 6)
    flag = lambda: rng.choice([True, True, False, None, 'absent'])

    def flagged(target, key):
        value = flag()
        if value != 'absent':
            target[key] = value

    def detail(vendor):
        data = {'id': vendor, 'name': rng.choice(['Poranna Snowbraid', 'Outfitter Eric', 'Élise', 'burbik']) + f' {vendor}',
                'zone': rng.choice(['Ironforge', 'Stormwind City', 'Orgrimmar', 'Dun Morogh', 'The Barrens']),
                'mapId': rng.choice([87, 84, None, '1'])}
        for axis in ('x', 'y'):
            data[axis] = rng.choice([None, 0, 100, 43.4, 29, round(rng.uniform(0, 100), 1)])
        side = rng.choice(['Alliance', 'Horde', 'Both', None, 'absent'])
        if side != 'absent':
            data['side'] = side
        title = rng.choice(['Tailoring Supplies', '', None, 'absent'])
        if title != 'absent':
            data['title'] = title
        flagged(data, 'seenInForever')
        flagged(data, 'classicData')
        sells = []
        for item in rng.sample(item_ids, rng.randrange(0, 4)):
            stack = rng.choice([1, 1, 1, 3, 5, 20])
            price = rng.choice([None, 0, 50, 536, 10, 12, 99999, stack * rng.randrange(1, 500)])
            sale = {'itemId': item, 'price': price, 'stack': stack, 'limited': rng.random() < 0.3,
                    'pricePerItem': rng.choice([0.5, 3.3333333333, 50, 1e-05])}
            name = rng.choice([f'Item {item}', None, 'absent'])
            if name != 'absent':
                sale['name'] = name
            stock = rng.choice([None, 0, 2, 'absent'])
            if stock != 'absent':
                sale['limitedStockCount'] = stock
            flagged(sale, 'seenInForever')
            flagged(sale, 'classicData')
            sells.append(sale)
        data['sells'] = sells
        return {'data': data}

    build = rng.choice(['1.60.1.70938', 'fixture'])
    pages = [{'data': [{'id': n, 'name': 'listed'} for n in ids[start:start + 100]],
              'meta': {'build': build, 'total': count, 'limit': 100, 'offset': start,
                       'next': start + 100 if start + 100 < count else None}}
             for start in range(0, max(count, 1), 100)]
    return {'pages': pages, 'details': {str(n): detail(n) for n in ids}}, item_ids


def sync_script(rng):
    """Scripted ForeverDB responses for one attempt, keyed by request index."""
    script = {}
    for _ in range(rng.choice([0, 0, 0, 1, 1, 2])):
        script[rng.choice([1, 1, 2, 3, 5, 9, 20, 36])] = rng.choice([
            {'status': 502, 'body': {'retryable': True, 'retry_after': rng.choice([0, 30, 60, 75, 90.5, -5, 'soon'])}},
            {'status': 502, 'body': {'retryable': True}},
            {'status': 502, 'body': {'retryable': False}},
            {'status': 502, 'body': 'upstream unavailable'},
            {'status': 503, 'body': {'echo': 'secret-fixture'}},
            {'status': 401, 'body': {'error': 'unauthorized'}},
            {'status': 429, 'body': {}},
            {'status': 302, 'headers': {'Location': '/elsewhere'}},
            {'body': 'not json at all'},
            {'body': {}},
            {'body': {'data': {'id': 'x'}, 'meta': {}}},
        ])
    return script


def sync_case(engine, rng, label, commands):
    routes = {'/': (200, ''), '/characters': (200, {'characters': [], 'sources': []}),
              '/listings': (200, {'listings': [], 'sources': []}),
              '/events': (200, {'events': [], 'next_cursor': None, 'checkpoint': 'terminal', 'has_more': False, 'sources': []}),
              f'/v1/pricetable/{MARKET}': (200, f'market|version|{int(T0.timestamp())}|2')}
    for prof in CATALOGUE_PAGES:
        routes[f'/professions/{prof}/'] = (200, profession_page(rng, prof))
    vocabulary = types.SimpleNamespace(routes=routes, requests=[], event_contract=None)
    fixture = Fixture(vocabulary)
    fixture.forever, item_ids = vendor_dataset(rng)
    try:
        with tempfile.TemporaryDirectory(prefix='everledger-fuzz-') as directory, \
                tempfile.TemporaryDirectory(prefix='everledger-fuzz-key-') as keys:
            env = {k: v for k, v in os.environ.items() if not k.startswith('EVERLEDGER_')}
            env.update(TZ='UTC', EVERLEDGER_ROSTER_TOKEN='fixture')
            credential = rng.choice(['env', 'env', 'env', 'file', 'none'])
            if credential == 'env':
                env['EVERLEDGER_FOREVERDB_API_KEY'] = rng.choice(['secret-fixture', ' secret-fixture \n'])
            elif credential == 'file':
                # Outside the data directory, whose every file is checked for the key.
                Path(keys, 'key').write_text(' secret-fixture\n')
                env['EVERLEDGER_FOREVERDB_API_KEY_FILE'] = str(Path(keys, 'key'))
            engine.start_scenario()
            attempt = Path(directory, 'vendors/foreverdb.net/attempt.json')
            step = 0
            for _ in range(rng.randrange(2, 6)):
                fixture.scripts = sync_script(rng)
                for argv in ([['refresh', 'catalogue', '--json']]
                             + [['vendors', str(rng.choice(item_ids + [999999])), *rng.choice([[], ['--json'], ['--in', 'IF']])]
                                for _ in range(rng.choice([0, 1, 2]))]
                             + ([['status', '--json']] if rng.random() < 0.3 else [])):
                    if commands and argv[0] not in commands:
                        continue
                    engine.step(f'{label}/{step}', argv, env, fixture, directory, forbidden=('secret-fixture',))
                    step += 1
                if attempt.exists() and rng.random() < 0.75:
                    state = json.loads(attempt.read_text())
                    state.update(attempted_at='2020-01-01T00:00:00Z', finished_at='2020-01-01T00:00:00Z')
                    attempt.write_text(json.dumps(state))
    finally:
        fixture.close()


def run(candidate, cases, seed, commands, only, invocations, suite):
    engine = Engine(candidate)
    failures = []
    try:
        for case in range(cases):
            label = f'fuzz-{suite}/{seed}/{case}'
            if only and only != label:
                continue
            rng = random.Random(seed * 100003 + case)
            if suite == 'sync':
                try:
                    sync_case(engine, rng, label, commands)
                except Exception as error:
                    failures.append(f'{label}: harness failure: {error!r}')
                continue
            routes, contract, cat, items, names = world(rng)
            vocabulary = types.SimpleNamespace(routes=routes, requests=[], event_contract=contract)
            fixture = Fixture(vocabulary)
            cursors = {'after': [], 'page': []}
            with tempfile.TemporaryDirectory(prefix='everledger-fuzz-') as directory:
                Path(directory, 'catalogue.json').write_text(json.dumps(cat, indent=1, ensure_ascii=False))
                env = {k: v for k, v in os.environ.items() if not k.startswith('EVERLEDGER_')}
                env.update(TZ=rng.choice(ZONES), EVERLEDGER_ROSTER_TOKEN='fixture')
                engine.start_scenario()
                try:
                    for step in range(invocations):
                        argv = invocation(rng, items, names, cat, cursors)
                        if rng.random() < 0.5:
                            argv.append('--json')
                        if commands and argv[0] not in commands:
                            continue
                        result = engine.step(f'{label}/{step}', argv, env, fixture, directory)
                        text = result.stdout if '--json' in argv else engine.records[-1]['oracle']['stdout'].decode()
                        if argv[0] == 'events' and text.startswith('{'):
                            data = json.loads(text)
                            for key, bucket in (('after_cursor', 'after'), ('next_cursor', 'page')):
                                if isinstance(data.get(key), str) and data[key]:
                                    cursors[bucket].append(data[key])
                except Exception as error:  # a harness failure is a finding too
                    failures.append(f'{label}: harness failure: {error!r}')
                finally:
                    fixture.close()
    finally:
        engine.close()
    return engine, failures


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('candidate')
    parser.add_argument('--cases', type=int, default=40)
    parser.add_argument('--seed', type=int, default=1)
    parser.add_argument('--invocations', type=int, default=14)
    parser.add_argument('--commands', default='', help='comma-separated commands to keep')
    parser.add_argument('--only', default='', help='one case label, for example fuzz-read/1/17')
    parser.add_argument('--suite', choices=['read', 'sync'], default='read',
                        help='read: catalogue, roster, market, vendor and journal commands; '
                             'sync: catalogue build and ForeverDB sync attempts')
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    candidate = Path(args.candidate).resolve()
    if not candidate.is_file():
        parser.error('candidate does not exist: ' + str(candidate))
    engine, failures = run([str(candidate)], args.cases, args.seed, set(filter(None, args.commands.split(','))),
                           args.only, args.invocations, args.suite)
    print(f'{"command":<12} {"cases":>7} {"identical":>10} {"sanctioned":>11} {"unexplained":>12}')
    for name, counts in sorted(engine.scoreboard().items()):
        print(f'{name:<12} {counts["cases"]:7} {counts["identical"]:10} {counts["sanctioned"]:11} {counts["unexplained"]:12}')
    shown = 0
    for row in engine.records:
        if row['state'] == 'unexplained' and shown < 40:
            shown += 1
            fields = ', '.join(d['field'] for d in row['differences'] if d['class'] is None)
            print(f"{row['name']}: {' '.join(row['oracle']['argv'])} [{fields}]")
    for failure in failures + engine.errors:
        print(failure, file=sys.stderr)
    if args.report:
        args.report.write_text(json.dumps(jsonable({'steps': engine.records, 'errors': failures + engine.errors}), indent=1))
    unexplained = sum(row['state'] == 'unexplained' for row in engine.records)
    print(f'steps {len(engine.records)}, unexplained {unexplained}, harness failures {len(failures) + len(engine.errors)}')
    return 1 if unexplained or failures or engine.errors else 0


if __name__ == '__main__':
    sys.exit(main())
