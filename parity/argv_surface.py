#!/usr/bin/env python3
"""Compare the argument surface of the oracle's argparse with a candidate binary.

Usage: python3 parity/argv_surface.py CANDIDATE

Each argv is classified on both sides as rejected by the argument parser itself,
answered with help, or accepted. What an accepted command then does is the
differential runner's concern. Every source URL points at a closed loopback
port, so nothing leaves the machine. Exits 1 when any classification differs.
"""
import json, os, random, shutil, socket, subprocess, sys, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CANDIDATE = sys.argv[1]
rng = random.Random(7)

COMMANDS = {
    'price': (['2604'], ['--json']),
    'recipe': (['Red', 'Shirt'], ['--json', '--skill 15']),
    'bridge': (['tailoring', '1', '20'], ['--json']),
    'margins': ([], ['--json', '--level 5', '--char Slade', '--known Slade']),
    'vendors': (['2604'], ['--json', '--in IF', '--char Slade', '--all']),
    'char': ([], ['--json', '--skill 5']),
    'listings': ([], ['--json', '--char Slade', '--item 2604']),
    'positions': ([], ['--json', '--char Slade', '--item 2604']),
    'sales': ([], ['--json', '--item 2604', '--char Slade', '--since 7d', '--until 1d', '--venue auction']),
    'purchases': ([], ['--json', '--item 2604', '--char Slade', '--since 7d', '--until 1d', '--venue vendor']),
    'expirations': ([], ['--json', '--item 2604', '--char Slade', '--since 7d', '--until 1d']),
    'net': (['2604'], ['--json', '--char Slade', '--since 7d', '--until 1d']),
    'events': ([], ['--json', '--item 2604', '--char Slade', '--since 7d', '--until 1d', '--venue auction',
                    '--type sale', '--cursor abc', '--after abc', '--limit 5']),
    'status': ([], ['--json']),
    'refresh': ([], ['--json']),
    'verify': ([], ['--json']),
}
cases = [[], ['-h'], ['--help'], ['--json'], ['--json', 'price', '2604'], ['bogus'], ['--version'], ['-V'],
         ['price'], ['Price', '2604'], ['pri', '2604'], ['--', 'price', '2604'], ['price', '--', '2604'],
         ['price', '--', '--json'], ['help'], ['help', 'price'], ['price', '2604', '--json=true'], ['price', '-j', '2604']]
for name, (positionals, flags) in COMMANDS.items():
    base = [name] + positionals
    cases += [base, base + ['-h'], base + ['--help'], [name, '--help'], base + ['--bogus'], base + ['extra'],
              base + ['--json', '--json'], ['--json'] + base, base + ['--jso'], base + ['-x'], base + ['--'],
              base + ['--', 'tail'], [name] + ['--json'] + positionals, base + ['--json=1'], base + ['-'],
              [name, '-1'], base + ['-1'], base + ['-5.5'], base + ['--no-json']]
    for flag in flags:
        parts = flag.split()
        cases.append(base + parts)
        cases.append([name] + parts + positionals)
        if len(parts) == 2:
            key, value = parts
            cases += [base + [key], base + [key + '=' + value], base + [key, value, key, value + 'x'],
                      base + [key[:-1], value], base + [key, ''], base + [key + '='], base + [key, '-x'],
                      base + [key, '-1'], base + [key, '--json'], base + [key, '--', value], base + [key.upper(), value],
                      base + [key, ' ' + value], base + [key, value + ' ']]
    for count in (2, 3):
        for _ in range(6):
            chosen = rng.sample(flags, min(count, len(flags)))
            argv = [name] + positionals + [p for f in chosen for p in f.split()]
            cases.append(argv)
            shuffled = [name] + [p for f in chosen for p in f.split()] + positionals
            cases.append(shuffled)
cases += [['events', '--limit', v] for v in ['0', '-1', '-0', '+5', ' 5', '5 ', '1_000', '5.0', 'abc', '', '٣', '0x10', '1e3', '99999999999999999999']]
cases += [['recipe', 'Red', '--skill', v] for v in ['15', '-3', '+3', 'x', '1.5', '']]
cases += [['bridge', 'tailoring'] + list(p) for p in [('1',), ('1', '20', '30'), ('a', '20'), ('1', 'b'), ('-1', '20'),
                                                       ('+1', '20'), ('1', '-20'), (' 1', '20'), ('1.0', '20'), ('', '20')]]
cases += [['sales', '--venue', v] for v in ['auction', 'vendor', 'both', 'AUCTION', '', 'auc']]
cases += [['events', '--venue', v] for v in ['auction', 'vendor', 'both', '']]
cases += [['refresh'] + list(p) for p in [(), ('catalogue',), ('catalog',), ('vendors',), ('catalogue', 'x'), ('',), ('Catalogue',)]]
cases += [['vendors', 'Red', '--in', 'IF', 'Dye'], ['vendors', '--in', 'IF', 'Red', 'Dye'], ['vendors', 'Red', 'Dye', '--in', 'IF'],
          ['vendors', '--all', 'Red', '--json', 'Dye'], ['recipe', '--skill', '5', 'Red', 'Shirt'], ['recipe', 'Red', '--skill', '5', 'Shirt'],
          ['net', 'Red', '--char', 'Slade', 'Dye'], ['price', 'Red', '--json', 'Dye'], ['char', 'Slade', '--skill', '5', 'tailoring'],
          ['char', '--skill', '5', 'Slade', 'tailoring'], ['char', 'Slade', 'tailoring', 'extra'], ['margins', 'tailoring', '12'],
          ['margins', '--level', '5', 'tailoring'], ['margins', 'tailoring', '--level', '5'], ['char', 'Slade', 'auctions'],
          ['events', '--type', 'sale,purchase'], ['events', '--type', ''], ['events', '--cursor', ''], ['events', '--after', ''],
          ['events', '--since'], ['events', '--since', '--until', '7d'], ['sales', '--item'], ['sales', '--item', '-x'],
          ['vendors', '--in'], ['vendors', '--in', '--all', '2604'], ['price', '-2604'], ['price', '--2604'], ['net', '-1'],
          ['vendors', '2604', '--in=IF', '--all'], ['vendors', '2604', '--in='], ['price', '2604', '--json', 'extra']]
seen, unique = set(), []
for argv in cases:
    key = tuple(argv)
    if key not in seen:
        seen.add(key)
        unique.append(argv)

sock = socket.socket(); sock.bind(('127.0.0.1', 0)); dead = f'http://127.0.0.1:{sock.getsockname()[1]}'; sock.close()
tmp = tempfile.mkdtemp(prefix='argv-surface-')
shutil.copy(ROOT / 'tests/fixtures/catalogue-fixture.json', Path(tmp) / 'catalogue.json')
env = dict(os.environ, TZ='UTC', EVERLEDGER_DATA_DIR=tmp, EVERLEDGER_ROSTER_URL=dead, EVERLEDGER_ROSTER_TOKEN='fixture',
           EVERLEDGER_AHLEDGER_URL=dead, EVERLEDGER_VENDOR_URL=dead, HTTP_PROXY=dead, HTTPS_PROXY=dead,
           NO_PROXY='127.0.0.1,localhost')
for key in ('EVERLEDGER_FOREVERDB_API_KEY', 'EVERLEDGER_FOREVERDB_API_KEY_FILE', 'EVERLEDGER_FOREVERDB_VIA_CONNECTOR',
            'EVERLEDGER_DYNAMIC_CREDENTIALS_DIR'):
    env.pop(key, None)


def run(binary, argv):
    r = subprocess.run([binary, *argv], env=env, capture_output=True, text=True, timeout=120)
    return r.returncode, r.stdout, r.stderr


def classify_oracle(code, out, err):
    if code == 2 and err.startswith('usage:'):
        return 'parser-reject'
    if code == 0 and out.startswith('usage:'):
        return 'help'
    return 'accepted'


def classify_candidate(code, out, err):
    if code == 2 and (err.startswith('error: ') and "try '--help'" in err or 'Usage:' in err or 'usage:' in err):
        return 'parser-reject'
    if code == 0 and ('Usage:' in out or 'usage:' in out):
        return 'help'
    return 'accepted'


results, mismatches = [], []
for argv in unique:
    code, out, err = run(str(ROOT / 'everledger'), argv)
    ccode, cout, cerr = run(CANDIDATE, argv)
    row = {'argv': argv, 'oracle': classify_oracle(code, out, err), 'oracle_code': code,
           'candidate': classify_candidate(ccode, cout, cerr), 'candidate_code': ccode}
    results.append(row)
    if row['candidate'] != row['oracle']:
        mismatches.append(row)
shutil.rmtree(tmp, ignore_errors=True)
summary = {}
for row in results:
    summary[row['oracle']] = summary.get(row['oracle'], 0) + 1
print('cases', len(results), 'oracle classes', summary)
print('mismatches', len(mismatches))
for row in mismatches:
    print('  ', json.dumps(row))
sys.exit(1 if mismatches else 0)
