"""Real loopback HTTP with exact query routes, scripts, and held responses."""
import copy
import http.server
import json
from pathlib import Path
import threading
from urllib.parse import parse_qs, urlsplit

ROOT = Path(__file__).resolve().parents[1]
HEADERS = ('Authorization', 'User-Agent', 'X-API-Key', 'X-Surrogate-Test')


class Raw:
    """Bytes to write to the socket as they are, for responses that are not valid HTTP."""

    def __init__(self, data):
        self.data = data


class Fixture:
    def __init__(self, vocabulary):
        self.vocabulary = vocabulary
        self.forever = json.loads((ROOT / 'tests/fixtures/foreverdb.json').read_text())
        self.scripts = {}
        self.log = []
        self.errors = []
        self.held = threading.Event()
        self.release = threading.Event()
        owner = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                owner.log.append({'method': self.command, 'path': self.path,
                                  'headers': {key: self.headers.get(key) for key in HEADERS}})
                owner.vocabulary.requests.append(self.path)
                try:
                    status, payload, headers, held = owner.response(self.path)
                    if held:
                        owner.held.set()
                        if not owner.release.wait(25):
                            raise TimeoutError('held fixture response was never released')
                    if isinstance(payload, Raw):
                        self.wfile.write(payload.data)
                        self.close_connection = True
                        return
                    self.send_response(status)
                    self.send_header('Last-Modified', 'Sat, 14 Nov 2026 20:20:39 GMT')
                    for key, value in headers.items():
                        self.send_header(key, value)
                    self.end_headers()
                    raw = payload if isinstance(payload, bytes) else (payload if isinstance(payload, str) else json.dumps(payload)).encode()
                    self.wfile.write(raw)
                except (BrokenPipeError, ConnectionResetError):
                    pass
                except BaseException as error:
                    owner.errors.append(repr(error))
                    self.close_connection = True

            def log_message(self, *args):
                pass

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={'poll_interval': .02}, daemon=True)
        self.thread.start()
        self.base = 'http://127.0.0.1:' + str(self.server.server_port)

    def response(self, path):
        parsed = urlsplit(path)
        # Scripts count only requests in the ForeverDB namespace. Other commands
        # in refresh (the twelve HTML pages) keep their ordinary fixture routes.
        if parsed.path.startswith('/api/v1/'):
            index = sum(row['path'].startswith('/api/v1/') for row in self.log)
            script = self.scripts.get(index)
            if script:
                return script.get('status', 200), script.get('body', {}), script.get('headers', {}), script.get('hold', False)
            if parsed.path == '/api/v1/vendors':
                query = parse_qs(parsed.query)
                if query.get('limit') != ['100'] or 'offset' not in query:
                    return 400, {'error': 'limit=100 and offset required'}, {}, False
                offset = int(query['offset'][0])
                if offset % 100:
                    return 400, {'error': 'unaligned offset'}, {}, False
                return 200, self.forever['pages'][offset // 100], {}, False
            detail = self.forever['details'].get(parsed.path.rsplit('/', 1)[1])
            return (200, detail, {}, False) if detail else (404, {}, {}, False)
        if self.vocabulary.event_contract and parsed.path == '/events':
            status, payload = self.vocabulary.event_contract(parse_qs(parsed.query))
        else:
            status, payload = self.vocabulary.routes.get(path, self.vocabulary.routes.get(parsed.path, (404, {})))
        return status, payload, {}, False

    def reset(self):
        self.log = []
        self.errors = []
        self.held.clear()
        self.release.clear()

    def large(self, count=40, pages=1):
        template = self.forever['details']['8681']['data']
        ids = list(range(1, count + 1))
        self.forever['details'] = {str(n): {'data': dict(copy.deepcopy(template), id=n)} for n in ids}
        self.forever['pages'] = []
        for page in range(pages):
            selected = ids[page * 100:(page + 1) * 100] if pages > 1 else ids
            self.forever['pages'].append({'data': [{'id': n} for n in selected], 'meta': {
                'build': 'fixture', 'total': count, 'limit': 100, 'offset': page * 100,
                'next': (page + 1) * 100 if page + 1 < pages else None}})

    def close(self):
        self.release.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
