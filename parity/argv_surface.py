#!/usr/bin/env python3
"""Compare the argument surface of the oracle's argparse with a candidate binary.

Usage: python3 parity/argv_surface.py CANDIDATE

Each argv is classified on both sides as rejected by the argument parser itself,
answered with help, or accepted. What an accepted command then does is the
differential runner's concern. Every source URL points at a closed loopback
port, so nothing leaves the machine. Exits 1 when any classification differs
outside the sanctioned integer forms that `sanctioned` names.

The daily request cap is read from the environment before the arguments are.
Each value in `CAP_VALUES` is classified on both sides as refused or accepted,
and a refused value must end every argv with the same bytes on both sides.
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
            'EVERLEDGER_DYNAMIC_CREDENTIALS_DIR', 'EVERLEDGER_FOREVERDB_DAILY_REQUESTS'):
    env.pop(key, None)


def run(binary, argv, extra=None):
    r = subprocess.run([binary, *argv], env=dict(env, **(extra or {})), capture_output=True, text=True, timeout=120)
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


INTEGER_OPTIONS = ('--limit', '--skill', '--level')
I64 = range(-2**63, 2**63)


def sanctioned(argv):
    """Name the sanctioned reason a candidate may reject an argv that argparse accepts.

    The port types command-line integers as i64 written in ASCII digits. Python's
    int() also takes any Unicode decimal digit and has no upper bound.
    """
    values = []
    for index, arg in enumerate(argv):
        for option in INTEGER_OPTIONS:
            if arg == option and index + 1 < len(argv):
                values.append(argv[index + 1])
            elif arg.startswith(option + '='):
                values.append(arg.split('=', 1)[1])
    if argv[:1] == ['bridge']:
        values += argv[2:4]
    for value in values:
        try:
            number = int(value)
        except ValueError:
            continue
        if not value.isascii():
            return 'non-ASCII digits in an integer argument'
        if number not in I64:
            return 'integer argument beyond i64'
    return None


results, mismatches, exempt = [], [], []
for argv in unique:
    code, out, err = run(str(ROOT / 'everledger'), argv)
    ccode, cout, cerr = run(CANDIDATE, argv)
    row = {'argv': argv, 'oracle': classify_oracle(code, out, err), 'oracle_code': code,
           'candidate': classify_candidate(ccode, cout, cerr), 'candidate_code': ccode}
    results.append(row)
    if row['candidate'] == row['oracle']:
        continue
    reason = sanctioned(argv) if (row['oracle'], row['candidate']) == ('accepted', 'parser-reject') else None
    if reason:
        exempt.append(dict(row, sanctioned=reason))
    else:
        mismatches.append(row)

CAP = 'EVERLEDGER_FOREVERDB_DAILY_REQUESTS'
REFUSAL = (2, '', CAP + ' must be a positive integer\n')
CAP_VALUES = ['1', '36', '5000', ' 40 ', '\t40\n', '\x0b40', '40\xa0', '\u200940', '+40', '4_0', '0040', '4294967296',
              '9223372036854775807', '', ' ', 'invalid', '1.5', '40.', '0', '-0', '+0', '00', '-1', '-40', '1e3', '0x24',
              '4 0', '_40', '40_', '4__0', '+ 40', '--40', '+-40', '\x1c40', 'None', 'true', '36,000', '٤٠', '４０', '4٠',
              '9223372036854775808', '1' + '0' * 30]


def cap_class(result):
    if result == REFUSAL:
        return 'refused'
    return 'accepted' if result[0] == 0 and 'sage:' in result[1] else 'other'


def cap_sanctioned(value):
    """Name the sanctioned reason a candidate may refuse a cap that Python accepts."""
    try:
        number = int(value)
    except ValueError:
        return None
    if any(ch.isdigit() and not ch.isascii() for ch in value):
        return 'non-ASCII digits in the daily cap'
    if number not in I64:
        return 'daily cap beyond i64'
    return None


cap_rows, cap_exempt, cap_mismatches = [], [], []
for value in CAP_VALUES:
    row = {'value': value, 'oracle': cap_class(run(str(ROOT / 'everledger'), ['--help'], {CAP: value})),
           'candidate': cap_class(run(CANDIDATE, ['--help'], {CAP: value}))}
    cap_rows.append(row)
    if row['oracle'] == row['candidate'] != 'other':
        continue
    reason = cap_sanctioned(value) if (row['oracle'], row['candidate']) == ('accepted', 'refused') else None
    if reason:
        cap_exempt.append(dict(row, sanctioned=reason))
    else:
        cap_mismatches.append(row)
refused_everywhere = 0
for value in ('invalid', '0', ''):
    for argv in unique[:18] + unique[18::12]:
        refused_everywhere += 1
        outcomes = {'oracle': run(str(ROOT / 'everledger'), argv, {CAP: value}), 'candidate': run(CANDIDATE, argv, {CAP: value})}
        if not outcomes['oracle'] == outcomes['candidate'] == REFUSAL:
            cap_mismatches.append({'value': value, 'argv': argv, **{side: list(result) for side, result in outcomes.items()}})
shutil.rmtree(tmp, ignore_errors=True)
summary = {}
for row in results:
    summary[row['oracle']] = summary.get(row['oracle'], 0) + 1
print('cases', len(results), 'oracle classes', summary)
print('sanctioned', len(exempt))
for row in exempt:
    print('  ', json.dumps(row, ensure_ascii=False))
print('mismatches', len(mismatches))
for row in mismatches:
    print('  ', json.dumps(row, ensure_ascii=False))
cap_summary = {}
for row in cap_rows:
    cap_summary[row['oracle']] = cap_summary.get(row['oracle'], 0) + 1
print('daily cap values', len(cap_rows), 'oracle classes', cap_summary, '| refused value on', refused_everywhere, 'argvs')
print('sanctioned', len(cap_exempt))
for row in cap_exempt:
    print('  ', json.dumps(row, ensure_ascii=False))
print('mismatches', len(cap_mismatches))
for row in cap_mismatches:
    print('  ', json.dumps(row, ensure_ascii=False))
sys.exit(1 if mismatches or cap_mismatches else 0)
