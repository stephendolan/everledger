#!/usr/bin/env python3
"""Capture Python oracle behavior; Rust tests consume the committed outputs."""
import base64
import datetime as dt
import json
import os
from pathlib import Path
import sys
import time
import unicodedata
import urllib.parse

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'python'))
import everledger_core as core
OUT = ROOT / 'tests/golden'
OUT.mkdir(parents=True, exist_ok=True)

def write(name, data):
    (OUT / (name + '.json')).write_text(json.dumps(data, ensure_ascii=False, indent=1) + '\n')

payloads = [
    '{}', '[]', '{"b":1,"a":2,"b":3}',
    '{"float":1.0,"int":1,"negzero":-0.0,"big":123456789012345678901234567890}',
    '[1e-7,1e-6,1e15,1e16,1e20,1e21,1e23,2.2250738585072014e-308,5e-324,1.7976931348623157e308]',
    '[0.84551240822557006,1.00000000000000011102230246251565404236316680908203125,9007199254740993.0]',
    '{"é":"Straße · 東京 😀 \\n \\t \\u0000","x":[null,true,false,{},[]]}',
    '[NaN, Infinity, -Infinity, -0, 1e400]',
    '{"z":{"d":1,"c":[2,3]},"a":1,"z":{"new":2}}',
]
write('json', [dict(payload=s, stdout=json.dumps(json.loads(s), ensure_ascii=False),
    error=json.dumps(json.loads(s)), cursor=json.dumps(json.loads(s), separators=(',',':')),
    catalogue=json.dumps(json.loads(s), ensure_ascii=False, indent=1)) for s in payloads])
nums = [0,1,-1,True,False,0.0,-0.0,.1,-.1,.5,1.5,2.5,3.5,-.5,-1.5,2.675,
    9999.5,10000,10001,1000101,-1000101,1e-7,1e16,1e20,1e23,380.8,79.7,1.005]
write('numbers',[dict(value=n,text=str(n),human=core.human(n),**({'rounded':round(n),'money':core.money(round(n))} if -(2**63)<=round(n)<2**63 else {})) for n in nums])
strings=['','Hello WORLD','Straße','İIıi','ΣΟΣ','ΟΣ\u0301','ΟΣ Α','ΑΣ.','ΑΣ\u2019','ᾈ ﬃ K','東京😀','a/b?x=1 +%é~']
write('strings',[dict(value=s,lower=s.lower(),casefold=s.casefold(),quote=urllib.parse.quote(s,safe=''),
    urlencode=urllib.parse.urlencode({'q':s,'n':True,'f':1.0})) for s in strings])
write('base64',[dict(value=s,encoded=base64.urlsafe_b64encode(s.encode()).decode()) for s in strings])
decodes=['','Zg==','Zg','Zg===','Zm8=','Zm9v','-___','a===','!!!!','Z g==','AAAA=','====','Zg==x','Zg==\n','AB==','ABC=']
vec=[]
for s in decodes:
    try: v=base64.b64decode(s,altchars=b'-_',validate=True); result=list(v)
    except ValueError: result=None
    vec.append(dict(value=s,decoded=result))
write('decode',vec)
origin=dt.datetime(2026,11,15,12,0,0,123456,tzinfo=dt.timezone.utc)
core.now=lambda: origin
values=['2026-11-15','20261115','2026-W46-7','2026W467','2026-W46','2026-11-15T12:34',
'2026-11-15 12:34:56.123456789Z','20261115T123456,1234567+0230','2026-11-15x12:34:56',
'2026-11-15T12','2026-11-15T12.5','2026-11-15T12:34.5','2026-11-15T12:34:56+02:30:15.123456',
'2026-11-15T12:34:56+00:00:00.5','2026-11-15T12:34:56Z','2026-11-15T12:34:56z','2026-11-15T12:34:56.123456789012345678901234567890Z',
'2026-11-15T24:00','2026-11-15T12:60','2026-11-15T12:00:60','2026-02-29','2026-1-1','2026-319',
'2026-11-15T12:00Zjunk','2026-11-15T12:00:00.123456Z','2026-11-15T12:00:00.123457Z',
'1969-12-31T23:59:59.9Z','2026-11-01T01:30:00','2026-03-08T02:30:00','7d','24h','0h','bad','']
vec=[]
for zone in ['UTC','America/New_York','Asia/Kathmandu']:
    os.environ['TZ']=zone; time.tzset()
    for s in values:
        r=dict(tz=zone,value=s,age=core.age(s))
        try:
            stamp=core.instant(s); r.update(instant=stamp.isoformat(timespec='microseconds'),iso=core.iso(stamp),seconds=core.roster_seconds(s))
        except (ValueError,OverflowError,TypeError): r['instant']=None
        try:r['selector']=core.time_selector(s)
        except core.Failure:r['selector']=None
        vec.append(r)
write('time',vec)
# Exhaustive scalar mappings avoid a dependency on Rust's Unicode table version.
with (ROOT/'src/unicode.tsv').open('w') as out:
    for cp in range(0x110000):
        if 0xD800 <= cp <= 0xDFFF:continue
        s=chr(cp)
        if s.casefold()!=s:
            out.write(f'{cp:x}\t'+','.join(f'{ord(c):x}' for c in s.casefold())+'\n')
write('metadata',dict(python=sys.version,unicode=unicodedata.unidata_version,now=origin.isoformat()))
print('generated JSON, numbers, Unicode, URL, base64 and time vectors')
import contextlib
import io
import random
import struct
rng=random.Random(289)
payloads=[]
for _ in range(1000):
    n=struct.unpack('d',struct.pack('Q',rng.getrandbits(64)))[0]
    if not (-float('inf') < n < float('inf')):continue
    payloads.append(dict(payload=repr(n),stdout=json.dumps(n,ensure_ascii=False),error=json.dumps(n),cursor=json.dumps(n,separators=(',',':')),catalogue=json.dumps(n,ensure_ascii=False,indent=1)))
write('float_roundtrip',payloads)
source_cases=[[],[{'name':'A','updated_at':'2026-11-15T12:00:00Z','stale':True}],
    [{'file':'Journalator.lua','mtime':'2020-01-01T00:00:00Z','stale':True}],
    [{'name':'A','saved_at':'z','age_seconds':5},{'name':'A','saved_at':'a','age_seconds':20},{'name':'A','mtime':None,'age_seconds':None}],
    [{'file':'/a/A.lua','file_age_seconds':3,'detail':'Data from A; x'},{'file':'/b/B.lua','age_seconds':10,'detail':'Data from B; y'}],
    [{'name':'A','age_seconds':5,'file_age_seconds':None,'scan_age_seconds':None}],
    [{'name':'A','age_seconds':1.0},{'name':'B','age_seconds':1.25}],
    [{'name':'vendor','fetched_at':'2026-11-15T11:00:00Z','age_seconds':None}],
    [{'name':'scan','scanned_at':'2026-11-15T10:00:00Z','scan_age_seconds':None,'age_seconds':5}]]
vec=[]
for rows in source_cases:
    for family in [False,True]:
        families={'family':[{k:v for k,v in reversed(list(r.items()))} for r in rows]} if family else {}
        data={'sources':rows}
        for stale in [False,True]:
            output=io.StringIO()
            with contextlib.redirect_stdout(output):core.emit(data,['row'],False,stale=stale,source_families=families)
            vec.append(dict(data=data,stale=stale,families=families,output=output.getvalue()))
write('emit',vec)
print('generated randomized float and provenance vectors')
import runpy
import tempfile
ns=runpy.run_path(str(ROOT/'python/everledger'))
fixture=json.loads((ROOT/'tests/fixtures/catalogue-fixture.json').read_text())
null=json.loads(json.dumps(fixture));r=null['professions']['tailoring']['recipes'][0]
r['bands'].update(yellow=None,green=None,grey=None);r['source_type']=None
null['professions']['leatherworking']={'stated_count':1,'recipes':[{**r,'name':'Cloudy Gustwoven Boots','bands':{**r['bands'],'learn':55}}]}
vec=[]
with tempfile.TemporaryDirectory() as directory:
    core.DATA_DIR=Path(directory)
    for cat in [fixture,null]:
        (core.DATA_DIR/'catalogue.json').write_text(json.dumps(cat))
        for args in [['bridge','tailoring','1','20'],['bridge','TAILORING','1','40'],['bridge','bad','1','20'],['bridge','tailoring','20','1']]:
            for flags in [[],['--json']]:
                core.load_catalogue.cache_clear();stdout=io.StringIO();stderr=io.StringIO()
                with contextlib.redirect_stdout(stdout),contextlib.redirect_stderr(stderr):code=ns['main'](args+flags)
                vec.append(dict(catalogue=json.dumps(cat),args=args+flags,code=code,stdout=stdout.getvalue(),stderr=stderr.getvalue()))
write('commands',vec)
print(f'generated {len(vec)} end-to-end command vectors')
write('human',[dict(payload=json.dumps(v),output=core.human(v)) for v in [None,'',True,False,1,1.0,'hello','a\x00\b\f\u00a0\u200b']])
