#!/usr/bin/env python3
"""Intentionally broken candidates used by the harness's behavioral proof."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / 'python'))
mode, args = sys.argv[1], sys.argv[2:]
if mode in ('request-order', 'query', 'cooldown'):
    from parity import oracle
    oracle.install()
    import everledger_core as core
    original = core.request
    saved = {}
    if mode == 'cooldown':
        # An attempt that died mid-request is dated from its start, not from
        # its last request: the journal reader never sees last_request_at.
        import json
        import types
        import everledger_foreverdb as db

        def loads(text):
            value = json.loads(text)
            if isinstance(value, dict) and 'attempted_at' in value and not value.get('finished_at'):
                value.pop('last_request_at', None)
            return value
        db.json = types.SimpleNamespace(loads=loads, dump=json.dump)

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
