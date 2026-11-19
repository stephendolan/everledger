#!/usr/bin/env python3
"""Run every parity instrument against two seam builds and print one summary.

Usage: matrix.py CANDIDATE RELEASE_CANDIDATE [LOGDIR] [--only NAME ...] [--jobs N]

CANDIDATE          a parity-seams build (debug is fine)
RELEASE_CANDIDATE  an optimized parity-seams build, for the production-size check
LOGDIR             optional log directory; defaults to a fresh temporary directory

Exit status is 0 only when every selected instrument exits 0.
"""
import argparse
import concurrent.futures
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

FRACTIONAL = '2026-11-15T12:34:56.789012Z'


def instruments(candidate, release, logs):
    py = sys.executable
    return [
        ('corpus', [py, 'parity/run.py', candidate, '--report', str(logs / 'corpus.json')], {}),
        ('corpus-fractional', [py, 'parity/run.py', candidate, '--report', str(logs / 'corpus-fractional.json')],
         {'EVERLEDGER_PARITY_ORIGIN': FRACTIONAL}),
        ('self-suite', [py, '-m', 'unittest', 'parity.test_parity'], {}),
        ('argv-surface', [py, 'parity/argv_surface.py', candidate], {}),
        ('fuzz-read', [py, 'parity/fuzz.py', candidate, '--cases', '60', '--seed', '1',
                       '--report', str(logs / 'fuzz-read.json')], {}),
        ('fuzz-read-fractional', [py, 'parity/fuzz.py', candidate, '--cases', '60', '--seed', '2',
                                  '--report', str(logs / 'fuzz-read-fractional.json')],
         {'EVERLEDGER_PARITY_ORIGIN': FRACTIONAL}),
        ('fuzz-sync', [py, 'parity/fuzz.py', candidate, '--suite', 'sync', '--cases', '60', '--seed', '1',
                       '--report', str(logs / 'fuzz-sync.json')], {}),
        ('damage', [py, 'parity/damage.py', candidate], {}),
        ('interrupts', [py, 'parity/interrupts.py', candidate], {}),
        ('handoff', [py, 'parity/handoff.py', candidate], {}),
        ('tls', [py, 'parity/tls.py', candidate], {}),
        ('recovery', [py, 'parity/recovery.py', candidate], {}),
        ('scale', [py, 'parity/scale.py', release], {}),
    ]


def run(repo, logs, name, argv, extra):
    env = {k: v for k, v in os.environ.items() if not k.startswith('EVERLEDGER_')}
    env.update(extra)
    started = time.monotonic()
    with (logs / f'{name}.log').open('wb') as log:
        done = subprocess.run(argv, cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT)
    return name, done.returncode, time.monotonic() - started


def tail(path, lines=3):
    text = path.read_text(errors='replace').rstrip().splitlines()
    return [line[:200] for line in text[-lines:]]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('candidate', type=Path)
    parser.add_argument('release', type=Path)
    parser.add_argument('logs', type=Path, nargs='?')
    parser.add_argument('--only', action='append', default=[])
    parser.add_argument('--jobs', type=int, default=4)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    logs = args.logs.resolve() if args.logs else Path(tempfile.mkdtemp(prefix='everledger-matrix-'))
    print(f'Logs: {logs}', flush=True)
    logs.mkdir(parents=True, exist_ok=True)
    selected = [row for row in instruments(str(args.candidate.resolve()), str(args.release.resolve()), logs)
                if not args.only or row[0] in args.only]
    if not selected:
        sys.exit('no instrument selected')
    results = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for name, code, seconds in pool.map(lambda row: run(repo, logs, *row), selected):
            results[name] = (code, seconds)
            print(f'{"ok  " if code == 0 else "FAIL"} {name:<22} exit {code}  {seconds:6.0f}s', flush=True)
    print()
    for name, (code, _) in results.items():
        print(f'--- {name} (exit {code})')
        for line in tail(logs / f'{name}.log'):
            print('    ' + line)
    failed = [name for name, (code, _) in results.items() if code]
    print()
    print('MATRIX ' + ('GREEN' if not failed else 'RED: ' + ', '.join(failed)))
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
