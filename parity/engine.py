"""Capture raw transcripts and compare only under the shared parity policy."""
import base64
import copy
import difflib
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import tempfile

from parity.oracle import ROOT, loopback

ORIGIN = os.environ.get('EVERLEDGER_PARITY_ORIGIN', '2026-11-15T12:00:00Z')
# All policy exemptions live here. Selection is explicit per step or based on
# the oracle's diagnostic grammar, never on the candidate's diagnostic text.
ALLOWLIST = {
    'parser-usage': {'stderr'},
    'python-internal': {'stderr'},
    'external-tool': {'stderr'},
    'shape-drift': {'stdout', 'stderr'},
    'help': {'stdout', 'stderr'},
}


def snapshot(directory):
    result = {}
    for path in sorted(Path(directory).rglob('*')):
        if path.is_symlink():
            raise ValueError('data snapshot contains a symlink: ' + str(path))
        if path.is_dir():
            result[path.relative_to(directory).as_posix() + '/'] = b''
        elif path.is_file():
            result[path.relative_to(directory).as_posix()] = path.read_bytes()
    return result


def modes(directory):
    """Permission bits of every regular file, keyed like the snapshot."""
    root = Path(directory)
    return {path.relative_to(root).as_posix(): path.stat().st_mode & 0o777
            for path in sorted(root.rglob('*')) if path.is_file() and not path.is_symlink()}


def restore(directory, files):
    root = Path(directory)
    for path in sorted(root.rglob('*'), reverse=True):
        if path.is_symlink() or path.is_file():
            path.unlink()
        elif path.is_dir():
            path.rmdir()
    for name, raw in files.items():
        path = root / name
        if name.endswith('/'):
            path.mkdir(parents=True, exist_ok=True)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)


SYNC_FILES = {f'vendors/foreverdb.net/{name}.json' for name in ('attempt', 'pending', 'index')}


def sync_spans(raw):
    """Locate only top-level UUID values, retaining the document's raw layout."""
    try:
        text = raw.decode('utf-8')
        decoder = json.JSONDecoder()
        position = len(text) - len(text.lstrip())
        if text[position] != '{':
            return []
        position += 1
        spans = []
        while True:
            position += len(text[position:]) - len(text[position:].lstrip())
            if text[position] == '}':
                return spans
            key, position = decoder.raw_decode(text, position)
            position += len(text[position:]) - len(text[position:].lstrip())
            if text[position] != ':':
                return []
            position += 1
            position += len(text[position:]) - len(text[position:].lstrip())
            start = position
            value, position = decoder.raw_decode(text, position)
            if key == 'sync_id' and isinstance(value, str) and re.fullmatch('[a-f0-9]{32}', value):
                spans.append((len(text[:start].encode()), len(text[:position].encode()), value.encode()))
            position += len(text[position:]) - len(text[position:].lstrip())
            if text[position] == '}':
                return spans
            if text[position] != ',':
                return []
            position += 1
    except (ValueError, IndexError):
        return []


def replace_sync(raw, replacement):
    for start, end, value in reversed(sync_spans(raw)):
        raw = raw[:start] + b'"' + replacement(value) + b'"' + raw[end:]
    return raw


def changes(before, after, target):
    target = dict(target)
    ids = {}
    for name in sorted(before.keys() & target.keys() & SYNC_FILES):
        for a, b in zip(sync_spans(before[name]), sync_spans(target[name])):
            ids[a[2]] = b[2]
    for name in before.keys() - after.keys():
        target.pop(name, None)
    for name, raw in after.items():
        if before.get(name) != raw:
            target[name] = replace_sync(raw, lambda value: ids.get(value, value)) if name in SYNC_FILES else raw
    return target


class Normalizer:
    def __init__(self):
        self.ids = {}

    def raw(self, raw):
        def replace(value):
            token = self.ids.setdefault(value, str(len(self.ids) + 1).encode())
            return b'<sync_id:' + token + b'>'
        return replace_sync(raw, replace)

    def transcript(self, transcript):
        result = copy.deepcopy(transcript)
        files = {}
        for path, raw in sorted(result['files'].items()):
            name = re.sub(r'^(vendors/foreverdb.net/(?:attempt|pending|index)\.json\.)[a-z0-9_]{8}(\.tmp)$', r'\1<temporary>\2', path)
            if name in files:
                raise ValueError('temporary filename normalization collision: ' + name)
            canonical = re.sub(r'\.<temporary>\.tmp$', '', name)
            files[name] = self.raw(raw) if canonical in SYNC_FILES else raw
        result['files'] = files
        return result


def byte_diff(left, right):
    # Prefix/suffix trimming keeps a one-byte mutation in a large cache cheap.
    prefix = 0
    while prefix < min(len(left), len(right)) and left[prefix] == right[prefix]:
        prefix += 1
    suffix = 0
    while suffix < min(len(left), len(right)) - prefix and left[-suffix - 1] == right[-suffix - 1]:
        suffix += 1
    a = left[prefix:len(left) - suffix if suffix else len(left)]
    b = right[prefix:len(right) - suffix if suffix else len(right)]
    # Each unequal byte range is represented, including insertions/deletions.
    return [{'oracle_offset': i + prefix, 'candidate_offset': k + prefix,
             'oracle': a[i:j], 'candidate': b[k:l]}
            for tag, i, j, k, l in difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes()
            if tag != 'equal']


def protected_stderr(raw):
    return [line for line in raw.splitlines(keepends=True)
            if line.startswith((b'deprecated ', b'cursor_invalid: resynchronizing ', b'catalogue is older than '))]


def compare(left, right, policy=None):
    differences = []
    for field in ('exit_code', 'stdout', 'stderr', 'requests', 'sleep_log'):
        if left[field] != right[field]:
            row = {'field': field, 'oracle': left[field], 'candidate': right[field]}
            if isinstance(left[field], bytes):
                row['byte_ranges'] = byte_diff(left[field], right[field])
            differences.append(row)
    for name in sorted(left['files'].keys() | right['files'].keys()):
        a, b = left['files'].get(name), right['files'].get(name)
        if a != b:
            row = {'field': 'files/' + name, 'oracle': a, 'candidate': b}
            if a is not None and b is not None:
                row['byte_ranges'] = byte_diff(a, b)
            differences.append(row)
    # restore() rewrites both sides' files with default permissions before each
    # step, so a mode differs only for a file the step itself wrote.
    left_modes, right_modes = left.get('modes', {}), right.get('modes', {})
    for name in sorted(left_modes.keys() & right_modes.keys()):
        if left_modes[name] != right_modes[name]:
            differences.append({'field': 'modes/' + name, 'oracle': oct(left_modes[name]),
                                'candidate': oct(right_modes[name])})
    if policy is not None and policy not in ALLOWLIST:
        raise ValueError('unknown parity policy: ' + policy)
    for row in differences:
        sanctioned = row['field'] in ALLOWLIST.get(policy, set())
        if policy == 'shape-drift' and not right['stderr'].strip():
            sanctioned = False
        if row['field'] == 'stderr' and protected_stderr(left['stderr']) != protected_stderr(right['stderr']):
            sanctioned = False
        if policy == 'help' and row['field'] == 'stdout':
            sanctioned = bool(left['stdout']) and bool(right['stdout'])
        if row['field'] == 'stderr' and left['stderr'].strip() and not right['stderr'].strip():
            sanctioned = False
        row['class'] = policy if sanctioned else None
    state = 'unexplained' if any(row['class'] is None for row in differences) else 'sanctioned' if differences else 'identical'
    return state, differences


def policy_for(args, oracle, explicit=None):
    if '--help' in args:
        return 'help'
    if explicit:
        return explicit
    text = oracle['stderr']
    if oracle['exit_code'] == 2 and text.startswith(b'usage:'):
        return 'parser-usage'
    if re.search(rb'(?m)^error: ', text) or re.search(rb'(?m)^Traceback \(most recent call last\):', text):
        return 'python-internal'
    if b'curl:' in text:
        return 'external-tool'
    return None


def jsonable(value):
    if isinstance(value, bytes):
        return {'base64': base64.b64encode(value).decode(), 'length': len(value)}
    if isinstance(value, dict):
        return {str(k): jsonable(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [jsonable(v) for v in value]
    return value


class Engine:
    def __init__(self, candidate, both=True):
        self.candidate = candidate
        self.both = both
        self.records = []
        self.errors = []
        self.previous = {}
        self.candidate_files = {}
        self.normalizers = [Normalizer(), Normalizer()]
        self.dead_proxy = socket.socket()
        self.dead_proxy.bind(('127.0.0.1', 0))  # bound, deliberately not listening

    def close(self):
        self.dead_proxy.close()

    def start_scenario(self):
        self.previous = {}
        self.candidate_files = {}
        self.normalizers = [Normalizer(), Normalizer()]

    def environment(self, given, fixture, directory, controls):
        env = {k: v for k, v in given.items() if k.startswith('EVERLEDGER_') or k in ('TZ', 'PATH', 'LANG', 'LC_ALL', 'SYSTEMROOT')}
        for key in list(env):
            if key.startswith(('EVERLEDGER_SEAM_', 'EVERLEDGER_PARITY_')):
                del env[key]
        for key in ('EVERLEDGER_ROSTER_URL', 'EVERLEDGER_AHLEDGER_URL', 'EVERLEDGER_VENDOR_URL'):
            env.setdefault(key, fixture.base)
        env['EVERLEDGER_SEAM_FOREVERDB_URL'] = fixture.base
        for key, value in env.items():
            if key.endswith('_URL'):
                loopback(value)
        proxy = 'http://127.0.0.1:' + str(self.dead_proxy.getsockname()[1])
        env.update(HTTP_PROXY=proxy, HTTPS_PROXY=proxy, ALL_PROXY=proxy,
                   http_proxy=proxy, https_proxy=proxy, all_proxy=proxy,
                   NO_PROXY='127.0.0.1,localhost,::1', no_proxy='127.0.0.1,localhost,::1',
                   PYTHONDONTWRITEBYTECODE='1', EVERLEDGER_DATA_DIR=str(directory),
                   EVERLEDGER_SEAM_NOW=ORIGIN, EVERLEDGER_SEAM_SLEEP_LOG=str(controls / 'sleeps'),
                   EVERLEDGER_PARITY_ELAPSED=str(controls / 'elapsed'))
        return env

    def capture(self, command, args, given, fixture, directory, initial, controls, death=None, plain=False):
        restore(directory, initial)
        fixture.reset()
        (controls / 'elapsed').write_text('0.0')
        (controls / 'sleeps').write_bytes(b'')
        env = self.environment(given, fixture, directory, controls)
        if plain:
            env = {k: v for k, v in env.items() if not k.startswith(('EVERLEDGER_SEAM_', 'EVERLEDGER_PARITY_'))}
        process = subprocess.Popen([*command, *args], cwd=ROOT, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, start_new_session=True)
        try:
            if death:
                if not fixture.held.wait(15):
                    raise TimeoutError('process did not reach held response')
                os.killpg(process.pid, death)
                # Wait until signal delivery has stopped the process before the
                # server is released; otherwise a response can win the race.
                stdout, stderr = process.communicate(timeout=15)
                fixture.release.set()
            else:
                stdout, stderr = process.communicate(timeout=30)
        except BaseException:
            os.killpg(process.pid, signal.SIGKILL)
            fixture.release.set()
            process.communicate()
            raise
        if fixture.errors:
            raise RuntimeError('fixture failed: ' + repr(fixture.errors))
        if process.returncode == 125:
            raise RuntimeError('oracle seam installation failed: ' + stderr.decode(errors='replace'))
        return {'argv': list(args), 'environment': env, 'exit_code': process.returncode,
                'stdout': stdout, 'stderr': stderr, 'requests': copy.deepcopy(fixture.log),
                'sleep_log': (controls / 'sleeps').read_bytes(), 'files': snapshot(directory),
                'modes': modes(directory)}

    def step(self, name, args, given, fixture, directory, death=None, policy=None, forbidden=()):
        requests_before = list(fixture.vocabulary.requests)
        before = snapshot(directory)
        self.candidate_files = changes(self.previous, before, self.candidate_files)
        candidate_before = dict(self.candidate_files)
        variants = [list(args)]
        if self.both and '--help' not in args:
            variants.append([a for a in args if a != '--json'] if '--json' in args else [*args, '--json'])
        replay = None
        with tempfile.TemporaryDirectory(prefix='everledger-parity-controls-') as temp:
            controls = Path(temp)
            for rendering, argv in enumerate(variants):
                # Fork normalization state for the extra rendering so UUIDs it
                # allocates cannot affect the next replayed invocation.
                norms = self.normalizers if rendering == 0 else copy.deepcopy(start_norms)
                if rendering == 0:
                    start_norms = copy.deepcopy(self.normalizers)
                a = self.capture([sys.executable, str(ROOT / 'parity/oracle.py')], argv,
                                 given, fixture, directory, before, controls, death)
                b = self.capture(self.candidate, argv, given, fixture, directory, candidate_before, controls, death)
                if '--help' in argv and (not a['stdout'] or not b['stdout']):
                    self.errors.append(name + ': help must have non-empty stdout')
                state, diffs = compare(norms[0].transcript(a), norms[1].transcript(b), policy_for(argv, a, policy))
                for sentinel in forbidden:
                    needle = sentinel.encode()
                    for side, transcript in (('oracle', a), ('candidate', b)):
                        if any(needle in raw for raw in [transcript['stdout'], transcript['stderr'], *transcript['files'].values()]):
                            self.errors.append(name + ': ' + side + ' leaked sentinel')
                self.records.append({'name': name + ('/alternate' if rendering else '/replay'),
                                     'command': next((arg for arg in argv if not arg.startswith('-')), 'help'),
                                     'state': state, 'differences': diffs, 'oracle': a, 'candidate': b})
                if rendering == 0:
                    replay = a
                    self.candidate_files = b['files']
        self.previous = replay['files']
        restore(directory, self.previous)
        # Existing test assertions inspect Handler.requests. Expose only the
        # replay oracle's requests, not the comparison or alternate rendering.
        fixture.vocabulary.requests = requests_before + [row['path'] for row in replay['requests']]
        return subprocess.CompletedProcess(argv, replay['exit_code'], replay['stdout'].decode(), replay['stderr'].decode())

    def scoreboard(self):
        result = {}
        for row in self.records:
            counts = result.setdefault(row['command'], dict(cases=0, identical=0, sanctioned=0, unexplained=0))
            counts['cases'] += 1
            counts[row['state']] += 1
        return result
