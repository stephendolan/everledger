"""Generate the reviewer-owned oracle probe corpus for the Rust primitives.

Run from the repository root:
    python3 scripts/gen_oracle_probe.py tests/probe/probe.jsonl

Every expected value comes from the live Python oracle or from the standard
library function the oracle calls. tests/oracle_probe.rs replays the corpus.
"""
from pathlib import Path

import base64, binascii, datetime as dt, json, math, os, random, struct, sys, time, urllib.parse

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'python'))
import everledger_core as core

rng = random.Random(20261115)
out = open(sys.argv[1], 'w', encoding='utf-8')
count = {}


def emit(kind, **fields):
    count[kind] = count.get(kind, 0) + 1
    out.write(json.dumps({'kind': kind, **fields}, ensure_ascii=True) + '\n')


# ---- floats -----------------------------------------------------------------
EDGE_FLOATS = [0.0, -0.0, 1.0, -1.0, 0.1, 0.2, 0.30000000000000004, 1 / 3, 2 / 3, 100.0, 1e15, 1e16,
               9999999999999998.0, 9007199254740992.0, 9007199254740993.0, 1e17, 1e21, 1e22, 1e23,
               1.5e-5, 0.0001, 0.00001, 0.00012345, 1e-4, 9.999e-5, 5e-324, 2.2250738585072014e-308,
               1.7976931348623157e308, 380.8, 79.7, 3.3333333333, 42.9, 28.3, 709.0, 0.95, 83.6,
               123456789.125, 1e100, 1.0000000000000002, 4.35, 2.675, 0.5, 1.5, 2.5, -0.5, -1.5,
               -2.5, 87.5, 88.5, 1234567.5, 4503599627370496.5, 0.49999999999999994, 5e-5, 12345678901234567.0]


def random_float():
    while True:
        x = struct.unpack('<d', rng.getrandbits(64).to_bytes(8, 'little'))[0]
        if math.isfinite(x):
            return x


floats = list(EDGE_FLOATS)
floats += [random_float() for _ in range(1500)]
floats += [rng.uniform(-1e6, 1e6) for _ in range(600)]
floats += [round(rng.uniform(0, 5000), rng.choice([1, 2, 3])) for _ in range(600)]
floats += [rng.randrange(-10**6, 10**6) + 0.5 for _ in range(400)]
floats += [10.0 ** e for e in range(-30, 31)] + [1.5 * 10.0 ** e for e in range(-30, 31)]
for k in range(-1074, 1024):
    x = math.ldexp(1.0, k)
    floats += [x, math.nextafter(x, math.inf), math.nextafter(x, 0.0)]
for x in floats:
    emit('float_repr', bits=struct.unpack('<Q', struct.pack('<d', x))[0], expected=repr(x))
    if abs(x) < 9e18:
        emit('py_round', bits=struct.unpack('<Q', struct.pack('<d', x))[0], expected=round(x))

# ---- money / human ----------------------------------------------------------
for v in [None, 0, 1, 99, 100, 101, 9999, 10000, 10001, 123456, -1, -100, -10001, 88, 536, 38000, 2094,
          0.4, 0.5, 1.5, 2.5, 99.5, 100.5, -0.5, -1.5, 87.5, 88.5, 9999.5, 10000.49, 1e6, 12345678.9]:
    emit('money', value=v, expected=core.money(v))
for v in [None, '', 'x', 'unknown', True, False, 0, 1, -3, 1.0, 709.0, 380.8, 1e16, 1e-5, ' ', 'None']:
    emit('human', value=v, expected=core.human(v))

# ---- casefold / lower -------------------------------------------------------
TEXTS = ['Red Dye', 'rEd dYe', 'STRASSE', 'Straße', 'ǅ', 'İstanbul', 'ΟΔΥΣΣΕΥΣ', 'ὈΔΥΣΣΕΎΣ', 'ΑΣ', 'Σ', 'ﬁre', 'ŉ',
          'ΐ', 'ᾈ', 'Ꭰ', 'ꭰ', 'ẞ', 'ſ', 'K', 'Å', 'Thunder Bluff', "White Swashbuckler's Shirt", 'ÉCOLE',
          'Ünïcödé', 'ΌΣΟΣ', 'ǰ', 'ΰ', 'ﬃ', 'ԵՒ', 'և', 'ᲐᲑ', '𐐀𐐁', 'İ', 'I', 'ı', 'Ⅷ', 'Ⓐ', 'μ', 'µ', '']
for cp in range(0x110000):
    c = chr(cp)
    if 0xD800 <= cp <= 0xDFFF:
        continue
    if c.casefold() != c or c.lower() != c:
        emit('casefold_cp', cp=cp, casefold=c.casefold(), lower=c.lower())
for t in TEXTS:
    emit('casefold', input=t, casefold=t.casefold(), lower=t.lower())
for _ in range(300):
    t = ''.join(chr(rng.choice([rng.randrange(0x20, 0x250), rng.randrange(0x370, 0x530),
                                rng.randrange(0x1E00, 0x2000), rng.randrange(0xFB00, 0xFB07)]))
                for _ in range(rng.randrange(1, 12)))
    emit('casefold', input=t, casefold=t.casefold(), lower=t.lower())

# ---- percent encoding -------------------------------------------------------
WIRE = ['Slade', 'Slade Undertow', 'opaque+/=', 'a&b=c', 'é', '日本', '~._-', "it's", 'a/b', '%41', ' ', '', '😀',
        'page:3', 'jnrc1:8:type=sale', 'x\ny', '\t', '"quoted"', '{}[]', '*', '@', '!$\'()*,;:']
for t in WIRE + [''.join(chr(rng.randrange(1, 0x300)) for _ in range(rng.randrange(1, 10))) for _ in range(200)]:
    emit('quote', input=t, expected=urllib.parse.quote(t, safe=''))
for _ in range(120):
    pairs = [[rng.choice(['limit', 'since', 'until', 'after', 'cursor', 'char', 'item']),
              rng.choice(WIRE + [rng.randrange(-5, 2000000000), rng.randrange(0, 600)])]
             for _ in range(rng.randrange(0, 5))]
    emit('urlencode', pairs=pairs, expected=urllib.parse.urlencode(dict(pairs)), dedup=list(dict(pairs).items()))

# ---- base64 -----------------------------------------------------------------
for n in list(range(0, 40)) + [rng.randrange(40, 300) for _ in range(40)]:
    raw = bytes(rng.getrandbits(8) for _ in range(n))
    emit('b64_encode', hex=raw.hex(), expected=base64.urlsafe_b64encode(raw).decode())
DECODE = ['', 'QQ==', 'QR==', 'QQ=', 'QQ', 'QUI=', 'QUJ=', 'QUJD', 'QUJDRA==', 'QUJD\n', ' QUJD', 'QU JD', 'QUJD=',
          'QUJD==', '====', 'Q===', 'QQ==QQ==', 'Q-_Q', 'Q+/Q', 'QUJD!', 'QUJDRA', 'QUJDR===', 'A', 'AA', 'AAA',
          'eyJjdXJzb3IiOiJwYWdlOjMiLCJ2aWV3Ijp7fSwidW50aWwiOm51bGx9', 'é', 'QUJD\x00', 'QUJDRA=', '=QUJ', 'QU=JD']
for t in DECODE:
    try:
        emit('b64_decode', input=t, ok=True, hex=base64.b64decode(t, altchars=b'-_', validate=True).hex())
    except (binascii.Error, ValueError) as error:
        emit('b64_decode', input=t, ok=False, error=type(error).__name__)

# ---- JSON -------------------------------------------------------------------
NUMS = ['0', '-0', '1', '-1', '0.0', '-0.0', '1.0', '1.50', '1E2', '1e2', '1e+2', '1E-2', '0.1e1', '100e-2', '1e16',
        '1e15', '123456789012345678', '9223372036854775807', '-9223372036854775808', '9223372036854775808',
        '18446744073709551615', '18446744073709551616', '123456789012345678901234567890', '0.30000000000000004',
        '2.2250738585072011e-308', '2.2250738585072012e-308', '1.7976931348623157e308', '4.9e-324', '5e-324',
        '0.1', '0.2', '3.3333333333', '380.8', '79.7', '42.9', '1e-7', '1e-5', '0.0001', '0.00001', '1e22', '1e23',
        '8.41e21', '9007199254740993', '9007199254740993.0', '0.000001', '123.456e5', '7.038531e-26',
        '1.00000000000000011102230246251565404236316680908203125', '0.500000000000000166533453693773481063544750213623046875',
        '6.631236871469758276785396630275967243399099947355303144249971758736286630139265439618068200788048744105960420552601852889715006376325666595539603330361800519107591783233358492337208057849499360899425128640718856616503093444922854759159988160304439909868291973931426625698663157749836252274523485312442358651207051292453083278116143932569727918709786004497872322193856150225415211997283078496319412124640111777216148110752815101775295719811974338451936095907419622417538473679495148632480391435931767981122396703443803335529756003353209830071832230689201383015598792184172909927924176339315507402234836120730914783168400715462440053817592702766213559042115986763819482654128770595766806872783349146967171293949598850675682115696218943412532098591327667236328125e-316']
STRS = ['', 'a', 'Red Dye', 'é', '日本語', '😀', 'line\nbreak', 'tab\there', 'quote"inside', 'back\\slash', '\u0000',
        '\u001f', '\u007f', '\u0080', ' ', ' ', '﻿', 'a/b', '</script>', 'Pattern · Drop', '×', '\b\f\r']


def rand_json(depth=0):
    roll = rng.random()
    if depth > 3 or roll < 0.45:
        return rng.choice([lambda: rng.choice(NUMS), lambda: json.dumps(rng.choice(STRS)), lambda: 'null',
                           lambda: 'true', lambda: 'false', lambda: str(rng.randrange(-10**6, 10**6)),
                           lambda: repr(round(rng.uniform(-1000, 1000), rng.randrange(0, 6)))])()
    if roll < 0.72:
        return '[' + rng.choice([',', ', ', ' , ']).join(rand_json(depth + 1) for _ in range(rng.randrange(0, 5))) + ']'
    keys = [rng.choice(STRS + ['id', 'name', 'count', 'sources', 'item', 'k1', 'k2', 'k3', 'id'])
            for _ in range(rng.randrange(0, 6))]
    return '{' + ','.join(json.dumps(k, ensure_ascii=rng.random() < 0.5) + rng.choice([':', ': ', ' : ']) + rand_json(depth + 1)
                          for k in keys) + '}'


docs = NUMS + [json.dumps(s) for s in STRS] + [json.dumps(s, ensure_ascii=False) for s in STRS]
docs += ['[]', '{}', '[[]]', '{"a":{}}', '{"a":[]}', '[{}]', '{"a":1,"b":2,"a":3}', '{"b":1,"a":{"d":1,"c":2}}',
         ' [ 1 , 2 ] ', '"\\ud83d\\ude00"', '"\\u00e9"', '"\\/"', '{"":1}', '[1.0,2,3.5,-0.0,1e2]',
         '{"x":NaN}', '[Infinity,-Infinity]', '"\\ud800"']
docs += [rand_json() for _ in range(900)]
for text in docs:
    try:
        value = json.loads(text)
    except ValueError as error:
        emit('json', input=text, ok=False)
        continue
    try:
        emit('json', input=text, ok=True,
             plain=json.dumps(value, ensure_ascii=False),
             ascii=json.dumps(value),
             compact_ascii=json.dumps(value, separators=(',', ':')),
             indent1=json.dumps(value, indent=1, ensure_ascii=False))
    except (ValueError, UnicodeError):
        emit('json', input=text, ok=False)

# ---- time -------------------------------------------------------------------
NOW = dt.datetime(2026, 11, 15, 12, 34, 56, 789012, tzinfo=dt.timezone.utc)
core.now = lambda: NOW
ZONES = ['UTC', 'America/New_York', 'Europe/London', 'Asia/Kolkata', 'Australia/Lord_Howe', 'Pacific/Kiritimati',
         'America/St_Johns', 'EST5EDT', 'Asia/Kathmandu']
STAMPS = ['2026-11-14', '2026-11-15', '2026-11-14T00:00:00Z', '2026-11-14T00:00:00', '2026-11-14 10:30', '2026-11-14T10',
          '2026-11-14T10:30', '2026-11-14T10:30:15', '2026-11-14T10:30:15.5', '2026-11-14T10:30:15.123456',
          '2026-11-14T10:30:15.1234567', '2026-11-14T10:30:15,25', '2026-11-14T10:30:15+00:00',
          '2026-11-14T10:30:15-05:00', '2026-11-14T10:30:15+0530', '2026-11-14T10:30:15+05', '2026-11-14T10:30:15+05:30:15',
          '2026-11-14t10:30:15z', '2026-11-14T10:30:15z', '20261114', '20261114T103015', '20261114T103015Z', '2026-W46-6',
          '2026-W46', '2026W466', '2026-11-14Z', '2026-318', '2026-11', '2026', 'yesterday', '', ' 2026-11-14', '2026-11-14 ',
          '2026-13-01', '2026-02-30', '2024-02-29', '2026-11-14T24:00:00', '2026-11-14T23:59:60', '0001-01-01', '9999-12-31',
          '9999-12-31T23:59:59Z', '0001-01-01T00:00:00Z', '1969-12-31T23:59:59.5Z', '1970-01-01T00:00:00Z',
          # US DST 2026: spring forward Mar 8 02:00->03:00, fall back Nov 1 02:00->01:00
          '2026-03-08T02:30:00', '2026-03-08T01:59:59', '2026-03-08T03:00:00', '2026-11-01T01:30:00', '2026-11-01T00:59:59',
          '2026-11-01T02:00:00', '2026-03-29T01:30:00', '2026-10-25T01:30:00', '2026-10-04T02:15:00', '2026-04-05T01:45:00',
          '2020-01-01T00:00:00Z', '2099-01-01', '2026-11-15T09:10:11-05:00', '2026-11-13T12:34:56.789012+00:00',
          '2026-11-14T10:30:15 Z', '2026-11-14T10:30:15ZZ', '１２', '2026-11-14T10:30:15.Z', '2026-11-14T1:30', '2026-1-4',
          '7d', '24h', '0d', '0h', '1d', '100d', '8760h', '739000d', '739900d', '740000d', '740500d', '800000d', '3652059d', '999999999d', '99999999999999999999h', '7D', '7 d', '7m', '-1d',
          '+1d', '1.5d', '٣d', '7d ', '07d', '0007h']
for zone in ZONES:
    os.environ['TZ'] = zone
    time.tzset()
    for stamp in STAMPS:
        try:
            moment = core.instant(stamp)
            emit('instant', tz=zone, input=stamp, ok=True, iso=core.iso(moment),
                 micros=(moment - dt.datetime(1970, 1, 1, tzinfo=dt.timezone.utc)) // dt.timedelta(microseconds=1))
        except (ValueError, OverflowError, TypeError) as error:
            emit('instant', tz=zone, input=stamp, ok=False, error=type(error).__name__)
        try:
            emit('time_selector', tz=zone, input=stamp, ok=True, expected=core.time_selector(stamp))
        except core.Failure as error:
            emit('time_selector', tz=zone, input=stamp, ok=False, code=error.code, message=str(error))
        emit('age', tz=zone, input=stamp, expected=core.age(stamp))
os.environ['TZ'] = 'UTC'
time.tzset()
emit('age', tz='UTC', input=None, expected=core.age(None))
for value in [0, 1794614400, '1794614400', '0', '2026-11-14T00:00:00Z', '2026-11-14', '1970-01-01T00:00:00.9Z',
              '1969-12-31T23:59:59.5Z', -5, '００', '12a', '', ' 12', '1e3', '2026-11-14T00:00:00-05:00']:
    try:
        emit('roster_seconds', value=value, ok=True, expected=core.roster_seconds(value))
    except (ValueError, OverflowError, TypeError) as error:
        emit('roster_seconds', value=value, ok=False, error=type(error).__name__)
for moment in [NOW, NOW.replace(microsecond=0), NOW.replace(microsecond=999999), dt.datetime(1969, 12, 31, 23, 59, 59, 500000, tzinfo=dt.timezone.utc)]:
    emit('iso', micros=(moment - dt.datetime(1970, 1, 1, tzinfo=dt.timezone.utc)) // dt.timedelta(microseconds=1), expected=core.iso(moment))

out.close()
print(json.dumps(count, indent=1))
