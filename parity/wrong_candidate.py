#!/usr/bin/env python3
"""Intentionally broken candidates used by the harness's behavioral proof."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
mode, args = sys.argv[1], sys.argv[2:]
if mode in ('request-order', 'query', 'cooldown'):
    from parity import oracle
    oracle.install()
    import everledger_core as core
    original = core.request
    saved = {}
    if mode == 'cooldown':
        import everledger_foreverdb as db
        read = db.read
        def ignore_last_start(name):
            value = read(name)
            if name == 'attempt.json' and value and 'finished_at' not in value:
                value.pop('last_request_at', None)
            return value
        db.read = ignore_last_start

    def request(url, headers=None):
        if mode == 'query' and '/events?' in url:
            return original(url + '&unexpected=1', headers)
        if mode == 'request-order':
            if url.endswith('/items/2604.json'):
                saved['pins'] = original(url.replace('/2604.json', '/2604.pins.json'), headers)
            elif url.endswith('/items/2604.pins.json') and 'pins' in saved:
                return saved.pop('pins')
        return original(url, headers)

    core.request = request
    sys.argv = [sys.argv[0], *args]
    oracle.main()
else:
    result = subprocess.run([sys.executable, str(ROOT / 'parity/oracle.py'), *args], capture_output=True)
    stdout, stderr, code = result.stdout, result.stderr, result.returncode
    if mode == 'stdout':
        stdout = stdout[:-1] + b'X' if stdout else b'X'
    elif mode == 'exit':
        code = (code + 1) % 5
    elif mode == 'stderr':
        stderr += b'contract corruption\n'
    elif mode == 'cache':
        path = Path(os.environ['EVERLEDGER_DATA_DIR']) / 'vendors/wowforever.ninja/2604.json'
        raw = path.read_bytes()
        path.write_bytes(b'[' + raw[1:])
    elif mode == 'sleep':
        with open(os.environ['EVERLEDGER_SEAM_SLEEP_LOG'], 'ab') as stream:
            stream.write(b'0.125\n')
    else:
        raise ValueError(mode)
    sys.stdout.buffer.write(stdout)
    sys.stderr.buffer.write(stderr)
    sys.exit(code)
