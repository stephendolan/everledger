#!/usr/bin/env python3
"""Run two candidate builds through the same harness and compare what each did, step by step.

Usage: compare_builds.py A B [LOGDIR] [--jobs N]

Both binaries must be files named `everledger` (the argument parser prints its own file name).
Runs the corpus and read fuzz at both clock origins, plus the sync fuzz, for each build.
Compares candidate transcripts without policy exemptions. Exit status 0 requires every pair
to be identical. Logs default to a fresh temporary directory.
"""
import argparse
import base64
import concurrent.futures
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

FRACTIONAL = '2026-11-15T12:34:56.789012Z'
REPO = Path(__file__).resolve().parents[1]

PORT = re.compile(rb'(127\.0\.0\.1|localhost|\[::1\]):\d+')
TEMP = re.compile(rb'/tmp/[A-Za-z0-9_.\-]+')
SYNC = re.compile(rb'"sync_id": "[0-9a-f]{32}"')
SUFFIX = re.compile(r'\.[0-9a-f]{8}\.tmp$|\.\d+\.tmp$|\.tmp[a-z0-9_]{8}$')


def mask(data):
    data = PORT.sub(rb'\1:PORT', data)
    data = TEMP.sub(b'/tmp/TEMP', data)
    return SYNC.sub(b'"sync_id": "SYNC"', data)


def raw(field):
    return mask(base64.b64decode(field['base64']))


def observed(side):
    requests = [(row['method'], PORT.sub(rb'\1:PORT', row['path'].encode()).decode(),
                 json.dumps(row.get('headers'), sort_keys=True)) for row in side['requests']]
    files = {SUFFIX.sub('.TMP', name): raw(body) for name, body in side['files'].items()}
    modes = {SUFFIX.sub('.TMP', name): mode for name, mode in side.get('modes', {}).items()}
    return {'exit_code': side['exit_code'], 'stdout': raw(side['stdout']), 'stderr': raw(side['stderr']),
            'requests': requests, 'sleep_log': raw(side['sleep_log']), 'files': files, 'modes': modes}


def load(path):
    with open(path) as stream:
        report = json.load(stream)
    return {step['name']: step for step in report['steps']}


def compare(base_path, new_path, show=5):
    base, new = load(base_path), load(new_path)
    only_base, only_new = sorted(set(base) - set(new)), sorted(set(new) - set(base))
    differing = []
    states = 0
    for name in sorted(set(base) & set(new)):
        a, b = observed(base[name]['candidate']), observed(new[name]['candidate'])
        fields = [field for field in a if a[field] != b[field]]
        if base[name]['state'] != new[name]['state']:
            states += 1
            fields.append('state')
        if fields:
            differing.append((name, fields, a, b))
    print(f'steps: base {len(base)}, new {len(new)}, shared {len(set(base) & set(new))}')
    print(f'only in base: {len(only_base)}; only in new: {len(only_new)}; differing: {len(differing)} '
          f'(of which {states} changed parity state)')
    for name in (only_base + only_new)[:show]:
        print('  unshared ' + name)
    for name, fields, a, b in differing[:show]:
        print(f'  DIFF {name}: {", ".join(fields)}')
        for field in fields:
            if field in ('stdout', 'stderr', 'sleep_log'):
                print(f'    base {field}: {a[field][:300]!r}')
                print(f'    new  {field}: {b[field][:300]!r}')
            elif field in ('exit_code', 'requests'):
                print(f'    base {field}: {str(a[field])[:300]}')
                print(f'    new  {field}: {str(b[field])[:300]}')
            elif field == 'files':
                for path in sorted(set(a['files']) | set(b['files'])):
                    if a['files'].get(path) != b['files'].get(path):
                        print(f'    file {path}: base {str(a["files"].get(path))[:200]!r}')
                        print(f'    file {path}: new  {str(b["files"].get(path))[:200]!r}')
    ok = not (only_base or only_new or differing)
    print('TRANSCRIPTS ' + ('IDENTICAL' if ok else 'DIFFER'))
    return ok


def suites(binary, out, side):
    py = sys.executable
    report = lambda name: str(out / f'{name}.{side}.json')
    return [
        ('corpus', [py, 'parity/run.py', binary, '--report', report('corpus')], {}),
        ('corpus-fractional', [py, 'parity/run.py', binary, '--report', report('corpus-fractional')],
         {'EVERLEDGER_PARITY_ORIGIN': FRACTIONAL}),
        ('fuzz-read', [py, 'parity/fuzz.py', binary, '--cases', '60', '--seed', '1', '--report', report('fuzz-read')], {}),
        ('fuzz-read-fractional', [py, 'parity/fuzz.py', binary, '--cases', '60', '--seed', '2',
                                  '--report', report('fuzz-read-fractional')], {'EVERLEDGER_PARITY_ORIGIN': FRACTIONAL}),
        ('fuzz-sync', [py, 'parity/fuzz.py', binary, '--suite', 'sync', '--cases', '60', '--seed', '1',
                       '--report', report('fuzz-sync')], {}),
    ]


def run(repo, out, side, name, argv, extra):
    env = {k: v for k, v in os.environ.items() if not k.startswith('EVERLEDGER_')}
    env.update(extra)
    with (out / f'{name}.{side}.log').open('wb') as log:
        code = subprocess.run(argv, cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT).returncode
    return side, name, code


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('old', type=Path)
    parser.add_argument('new', type=Path)
    parser.add_argument('out', type=Path, nargs='?')
    parser.add_argument('--jobs', type=int, default=6)
    args = parser.parse_args()
    repo = REPO
    out = args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix='everledger-builds-'))
    print(f'Logs: {out}', flush=True)
    out.mkdir(parents=True, exist_ok=True)
    for binary in (args.old, args.new):
        if binary.name != 'everledger':
            sys.exit(f'{binary} must be named everledger')
    work = [(side, *row) for side, binary in (('old', args.old), ('new', args.new))
            for row in suites(str(binary.resolve()), out, side)]
    failed = False
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for side, name, code in pool.map(lambda row: run(repo, out, *row), work):
            print(f'{"ok  " if code == 0 else "FAIL"} {side} {name} exit {code}', flush=True)
            failed |= code != 0
    for name in ('corpus', 'corpus-fractional', 'fuzz-read', 'fuzz-read-fractional', 'fuzz-sync'):
        print(f'--- {name}')
        failed |= not compare(out / f'{name}.old.json', out / f'{name}.new.json', show=4)
    print('BUILD PAIRS ' + ('DIFFER' if failed else 'IDENTICAL'))
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
