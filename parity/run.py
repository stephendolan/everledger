#!/usr/bin/env python3
"""One-command differential runner; JSON retains complete raw transcripts."""
import argparse
import fnmatch
import json
from pathlib import Path
import sys

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.corpus import TRANSLATIONS, accounting, load_cli, replay_cli, replay_db
from parity.engine import Engine, ROOT, jsonable


def run(candidate, patterns=(), both=True, progress=True):
    module = load_cli()
    inventory = accounting(module)
    engine = Engine(candidate, both=both)
    selected = []
    try:
        for row in inventory:
            name = row['scenarios'][0]
            if patterns and not any(fnmatch.fnmatch(name, pattern) or pattern in name for pattern in patterns):
                continue
            selected.append(name)
            if progress:
                print('running ' + name, flush=True)
            count_before = len(engine.records)
            try:
                if name.startswith('cli/'):
                    replay_cli(engine, module, name.split('/', 1)[1])
                else:
                    replay_db(engine, module, name.split('/', 1)[1])
                if len(engine.records) == count_before:
                    engine.errors.append(name + ': scenario made no CLI invocations')
            except Exception:
                import traceback
                engine.errors.append(name + ': harness failure\n' + traceback.format_exc())
        if not selected:
            engine.errors.append('no scenarios selected')
    finally:
        engine.close()
    return {'accounting': inventory, 'selected': selected, 'translations': TRANSLATIONS,
            'scoreboard': engine.scoreboard(), 'steps': engine.records, 'errors': engine.errors,
            'success': bool(selected) and not engine.errors and all(row['state'] != 'unexplained' for row in engine.records)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('candidate', help='candidate executable (built with parity-seams)')
    parser.add_argument('--scenario', action='append', default=[], help='substring or glob; repeatable')
    parser.add_argument('--report', type=Path, default=ROOT / 'parity/report.json')
    parser.add_argument('--list', action='store_true')
    args = parser.parse_args()
    if args.list:
        for row in accounting(load_cli()):
            print(row['test'] + ' -> ' + ', '.join(row['scenarios']))
        return 0
    candidate = Path(args.candidate).resolve()
    if not candidate.is_file():
        parser.error('candidate does not exist: ' + str(candidate))
    report = run([str(candidate)], args.scenario)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(jsonable(report), indent=2) + '\n')
    mapped = sum(bool(row['scenarios']) for row in report['accounting'])
    internal = sum(bool(row['not_cli_observable']) for row in report['accounting'])
    print(f'Accounting: {mapped} mapped + {internal} not CLI-observable = {len(report["accounting"])} tests; {len(report["selected"])} selected')
    print(f'{"command":<12} {"cases":>7} {"identical":>10} {"sanctioned":>11} {"unexplained":>12}')
    for name, counts in sorted(report['scoreboard'].items()):
        print(f'{name:<12} {counts["cases"]:7} {counts["identical"]:10} {counts["sanctioned"]:11} {counts["unexplained"]:12}')
    for row in report['steps']:
        if row['state'] != 'identical':
            print(row['name'] + ': ' + row['state'] + ' ' + ', '.join(d['field'] + (' [' + d['class'] + ']' if d['class'] else '') for d in row['differences']))
    for error in report['errors']:
        print(error, file=sys.stderr)
    print('report: ' + str(args.report))
    return 0 if report['success'] else 1


if __name__ == '__main__':
    sys.exit(main())
