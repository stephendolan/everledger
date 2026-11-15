#!/usr/bin/env python3
"""Run the read-only oracle with the three feature-gated candidate seams."""
import datetime
import ipaddress
import os
from pathlib import Path
import runpy
import sys
import time
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
REAL_DATETIME = datetime.datetime
_INSTALLED = False


def loopback(url):
    parsed = urlsplit(url)
    if parsed.scheme not in ('http', 'https') or parsed.username or parsed.password:
        raise ValueError('fixture URL must be HTTP(S) without credentials: ' + url)
    host = parsed.hostname
    if host != 'localhost' and not ipaddress.ip_address(host or '').is_loopback:
        raise ValueError('fixture URL must be loopback: ' + url)
    return url


def install():
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True
    origin = REAL_DATETIME.fromisoformat(os.environ['EVERLEDGER_SEAM_NOW'].replace('Z', '+00:00'))
    state = Path(os.environ['EVERLEDGER_PARITY_ELAPSED'])
    log = Path(os.environ['EVERLEDGER_SEAM_SLEEP_LOG'])

    def elapsed():
        return float(state.read_text())

    class ClockDateTime(REAL_DATETIME):
        @classmethod
        def now(cls, tz=None):
            wall = origin + datetime.timedelta(seconds=elapsed())
            return wall.astimezone(tz) if tz else wall.astimezone().replace(tzinfo=None)

        @classmethod
        def utcnow(cls):
            return cls.now(datetime.timezone.utc).replace(tzinfo=None)

    def sleep(seconds):
        seconds = float(seconds)
        if seconds < 0:
            raise ValueError('sleep length must be non-negative')
        with log.open('a') as stream:
            stream.write(repr(seconds) + '\n')
        state.write_text(repr(elapsed() + seconds))

    datetime.datetime = ClockDateTime
    time.monotonic = elapsed
    time.sleep = sleep
    import everledger_foreverdb as db
    db.BASE = loopback(os.environ['EVERLEDGER_SEAM_FOREVERDB_URL']).rstrip('/')


def main():
    os.environ['EVERLEDGER_PARITY_ORACLE'] = '1'
    paths = [str(ROOT / 'parity/oracle_hook'), str(ROOT)]
    os.environ['PYTHONPATH'] = os.pathsep.join(paths)
    sys.path.insert(0, str(ROOT))
    install()
    sys.argv = [str(ROOT / 'everledger'), *sys.argv[1:]]
    runpy.run_path(str(ROOT / 'everledger'), run_name='__main__')


if __name__ == '__main__':
    main()
