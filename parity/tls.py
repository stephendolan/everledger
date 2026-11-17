#!/usr/bin/env python3
"""Both implementations against a fixture that speaks TLS.

Usage: python3 parity/tls.py CANDIDATE

Every other check reaches its fixture over plain HTTP, so none of them runs a
handshake or verifies a certificate. This one mints a throwaway certificate
authority, gives the fixture a certificate for `localhost` signed by it, and
has both implementations fetch over TLS:

- **Trusted.** With the authority in `SSL_CERT_FILE`, both sides look a vendor
  up, quote a price, read status, rebuild the catalogue, and sync ForeverDB by
  key and by connector. They must exit the same, print the same, and send the
  same requests.
- **Untrusted.** Without the authority, each command that needs a source must
  end with exit code 4 on both sides, and no request may reach the fixture.
- **Wrong name.** With the authority trusted and the fixture addressed as
  `127.0.0.1`, which its certificate does not name, the same holds.

Needs the `openssl` command to mint the certificates. CANDIDATE needs the
`parity-seams` feature.
"""
import ssl
import subprocess
import sys
import tempfile
from pathlib import Path

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from parity.corpus import DBScenario, load_cli
from parity.engine import Engine, ROOT, snapshot

ORACLE = [sys.executable, str(ROOT / 'parity/oracle.py')]
REFRESH = ['refresh', 'catalogue', '--json']
LOOKUP = ['vendors', '2604', '--json']
PRICE = ['price', '2604', '--json']
STATUS = ['status', '--json']
AUTHORITY = """[req]
distinguished_name = name
x509_extensions = authority
prompt = no
[name]
CN = everledger parity authority
[authority]
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign
subjectKeyIdentifier = hash
"""
SERVER = """subjectAltName = DNS:localhost
basicConstraints = CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
"""
failures = []


def check(condition, message):
    print(('ok    ' if condition else 'FAIL  ') + message)
    if not condition:
        failures.append(message)


def mint(directory):
    """A certificate authority, and a certificate for `localhost` that it signed."""
    def openssl(*args):
        subprocess.run(['openssl', *args], cwd=directory, check=True, capture_output=True)
    Path(directory, 'authority.cnf').write_text(AUTHORITY)
    Path(directory, 'server.ext').write_text(SERVER)
    openssl('req', '-x509', '-config', 'authority.cnf', '-newkey', 'rsa:2048', '-nodes', '-days', '2',
            '-keyout', 'authority.key', '-out', 'authority.pem')
    openssl('req', '-new', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost',
            '-keyout', 'server.key', '-out', 'server.csr')
    openssl('x509', '-req', '-in', 'server.csr', '-CA', 'authority.pem', '-CAkey', 'authority.key',
            '-CAcreateserial', '-days', '2', '-extfile', 'server.ext', '-out', 'server.pem')
    return Path(directory, 'authority.pem'), Path(directory, 'server.pem'), Path(directory, 'server.key')


def speak_tls(fixture, certificate, key):
    """Put the fixture's listening socket behind TLS; the handshake runs as each connection is accepted."""
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certificate, key)
    fixture.server.socket = context.wrap_socket(fixture.server.socket, server_side=True)


def address(scenario, host):
    """Point every source of the scenario at the fixture under this host name."""
    base = f'https://{host}:{scenario.fixture.server.server_port}'
    scenario.fixture.base = base
    for key in ('EVERLEDGER_ROSTER_URL', 'EVERLEDGER_AHLEDGER_URL', 'EVERLEDGER_VENDOR_URL'):
        scenario.env[key] = base


def both(engine, scenario, candidate, argv, state):
    with tempfile.TemporaryDirectory() as controls:
        return [engine.capture(side, argv, scenario.env, scenario.fixture, scenario.root, state, Path(controls))
                for side in (ORACLE, candidate)]


def agree(engine, scenario, candidate, label, state, commands):
    for argv in commands:
        reference, port = both(engine, scenario, candidate, argv, state)
        same = all(reference[field] == port[field] for field in ('exit_code', 'stdout', 'requests', 'sleep_log'))
        check(same and reference['requests'],
              f'{label}: {" ".join(argv)} gives the same answer over TLS '
              f'(exit {reference["exit_code"]} and {port["exit_code"]}, {len(reference["requests"])} and {len(port["requests"])} requests)')


def refuse(engine, scenario, candidate, label, state, commands):
    for argv in commands:
        reference, port = both(engine, scenario, candidate, argv, state)
        for name, side in (('Python', reference), ('Rust', port)):
            check(side['exit_code'] == 4 and not side['requests'] and not side['stdout'],
                  f'{label}: {name} ends {" ".join(argv)} with exit code 4 and sends nothing '
                  f'(exit {side["exit_code"]}, {len(side["requests"])} requests)')


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    candidate = [str(Path(sys.argv[1]).resolve())]
    engine = Engine(candidate, both=False)
    module = load_cli()
    with tempfile.TemporaryDirectory(prefix='everledger-tls-') as directory:
        authority, certificate, key = mint(directory)
        scenario = DBScenario(engine, module, 'tls')
        try:
            speak_tls(scenario.fixture, certificate, key)
            fresh = snapshot(scenario.root)

            address(scenario, 'localhost')
            scenario.env['SSL_CERT_FILE'] = str(authority)
            engine.start_scenario()
            agree(engine, scenario, candidate, 'trusted, ForeverDB by key', fresh, [LOOKUP, PRICE, STATUS, REFRESH])
            scenario.connector()
            agree(engine, scenario, candidate, 'trusted, ForeverDB by connector', fresh, [REFRESH])
            scenario.env['EVERLEDGER_FOREVERDB_VIA_CONNECTOR'] = ''
            scenario.env['EVERLEDGER_FOREVERDB_API_KEY'] = 'secret-fixture'

            del scenario.env['SSL_CERT_FILE']
            refuse(engine, scenario, candidate, 'authority not trusted', fresh, [LOOKUP, PRICE, REFRESH])

            scenario.env['SSL_CERT_FILE'] = str(authority)
            address(scenario, '127.0.0.1')
            refuse(engine, scenario, candidate, 'certificate names another host', fresh, [LOOKUP, PRICE, REFRESH])
        finally:
            scenario.close()
            engine.close()
    print(f'{len(failures)} failures')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
