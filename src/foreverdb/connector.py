"""Isolated connector request: credentials and helper output stay in this process."""
import base64
import contextlib
import importlib
import json
import os
import sys
import urllib.error
import urllib.request

def perform():
    directory, slug, url, user_agent = sys.argv[1:]
    sys.path.insert(0, directory)
    try:
        helper = importlib.import_module('dynamic_credentials').add_surrogate_to_request
    except (ImportError, AttributeError):
        return {'kind': 'missing'}
    except BaseException:
        return {'kind': 'import'}
    if not url:
        return {'kind': 'ok'}
    request = urllib.request.Request(url, headers={'User-Agent': user_agent})
    try:
        modified = helper(request, slug, allowed_hosts=['foreverdb.net'])
        if modified is not None:
            request = modified
    except BaseException:
        return {'kind': 'prepare'}
    request.add_header('User-Agent', user_agent)
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args):
            return None
    try:
        with urllib.request.build_opener(NoRedirect()).open(request, timeout=20) as response:
            return {'kind': 'ok', 'body': base64.urlsafe_b64encode(response.read()).decode()}
    except urllib.error.HTTPError as error:
        try:
            payload = json.loads(error.read())
        except (ValueError, UnicodeError):
            payload = None
        retry = {key: payload[key] for key in ('retryable', 'retry_after') if key in payload} if isinstance(payload, dict) else None
        return {'kind': 'http', 'status': error.code, 'body': base64.urlsafe_b64encode(json.dumps(retry).encode()).decode()}
    except BaseException:
        return {'kind': 'transport'}

response_fd = os.dup(sys.stdout.fileno())
with open(os.devnull, 'w') as sink, contextlib.redirect_stdout(sink), contextlib.redirect_stderr(sink):
    os.dup2(sink.fileno(), 1)
    os.dup2(sink.fileno(), 2)
    try:
        reply = perform()
    except BaseException:
        reply = {'kind': 'transport'}
os.write(response_fd, json.dumps(reply).encode())
os.close(response_fd)
