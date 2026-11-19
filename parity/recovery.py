#!/usr/bin/env python3
"""Compare recovery after every request with both ForeverDB transports."""
import argparse
import signal
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.corpus import DBScenario, load_cli
from parity.engine import Engine


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('candidate', type=Path)
    args = parser.parse_args()
    engine = Engine([str(args.candidate.resolve())], both=False)
    try:
        module = load_cli()
        for connector in (False, True):
            for death in (signal.SIGINT, signal.SIGKILL):
                for request in range(1, 7):
                    scenario = DBScenario(engine, module,
                                          f'recovery-{connector}-{death}-{request}')
                    try:
                        if connector:
                            scenario.connector()
                        scenario.sync()
                        scenario.tomorrow()
                        scenario.fixture.scripts = {request: {'hold': True}}
                        scenario.sync(death=death,
                                      policy='python-internal' if death == signal.SIGINT else None)
                        scenario.fixture.scripts = {}
                        scenario.sync()
                        scenario.tomorrow()
                        scenario.sync()
                    finally:
                        scenario.close()
        for command, counts in engine.scoreboard().items():
            print(command, counts)
        failures = [row['name'] for row in engine.records if row['state'] == 'unexplained']
        for failure in failures:
            print(failure)
        for error in engine.errors:
            print(error)
        if failures or engine.errors:
            return 1
        print('24 signal/transport/request combinations: PASS')
        return 0
    finally:
        engine.close()


if __name__ == '__main__':
    sys.exit(main())
