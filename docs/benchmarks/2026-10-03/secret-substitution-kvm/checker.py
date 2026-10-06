#!/usr/bin/env python3
"""Verify daemon-scoped substitution from an owned KVM guest to owned HTTPS."""
import argparse
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import json
import os
from pathlib import Path
import secrets
import shlex
import signal
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.parse
import urllib.request


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('daemon', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    require(os.name == 'posix' and Path('/dev/kvm').exists(), 'Linux KVM fixture required')
    require(not args.output.exists(), 'report already exists')
    hashes = {name: digest(getattr(args, name)) for name in ('daemon', 'kernel', 'initrd')}
    interfaces = json.loads(subprocess.check_output(['ip', '-j', '-4', 'addr', 'show', 'scope', 'global'], timeout=5))
    addresses = [a['local'] for interface in interfaces for a in interface['addr_info'] if 'local' in a]
    require(addresses, 'owned host IPv4 address required')
    address = str(ipaddress.IPv4Address(addresses[0]))
    checks = []
    received = []
    process = None
    server = None
    thread = None
    with tempfile.TemporaryDirectory(prefix='hm-secret-kvm-') as directory:
        root = Path(directory)
        policy = root / 'policy.json'
        policy.write_text('{"version":1,"sandboxes":[]}'); policy.chmod(0o600)
        generated = subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ed25519', '-nodes',
                                    '-keyout', str(root / 'root.key'), '-out', str(root / 'root.pem'),
                                    '-days', '1', '-subj', '/CN=Owned HTTPS fixture root'],
                                   capture_output=True, timeout=20)
        require(generated.returncode == 0, 'owned certificate generation failed')
        for command in [
            ['openssl', 'req', '-new', '-newkey', 'ed25519', '-nodes', '-keyout', str(root / 'server.key'),
             '-out', str(root / 'server.csr'), '-subj', '/CN=Owned HTTPS fixture',
             '-addext', 'subjectAltName=DNS:api.secret.example.test,DNS:other.secret.example.test',
             '-addext', 'basicConstraints=critical,CA:FALSE'],
            ['openssl', 'x509', '-req', '-in', str(root / 'server.csr'), '-CA', str(root / 'root.pem'),
             '-CAkey', str(root / 'root.key'), '-CAcreateserial', '-copy_extensions', 'copy',
             '-days', '1', '-out', str(root / 'server.pem')]]:
            require(subprocess.run(command, capture_output=True, timeout=20).returncode == 0,
                    'owned leaf generation failed')
        (root / 'root.pem').chmod(0o600)
        class Handler(BaseHTTPRequestHandler):
            protocol_version = 'HTTP/1.1'
            def log_message(self, *arguments):
                pass
            def do_POST(self):
                length = int(self.headers.get('Content-Length', '0'))
                if length < 0 or length > 1024 * 1024:
                    self.send_error(413); return
                received.append({'path': self.path, 'headers': dict(self.headers), 'body': self.rfile.read(length)})
                self.send_response(200); self.send_header('Content-Length', '2'); self.end_headers(); self.wfile.write(b'OK')
        server = ThreadingHTTPServer((address, 0), Handler)
        server.daemon_threads = True
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(root / 'server.pem', root / 'server.key')
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        token = 'hms_' + 'a' * 64
        api_port = port()
        endpoint = f'http://127.0.0.1:{api_port}'
        cluster_token = secrets.token_hex(32)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def api(method, path, body=None):
            request = urllib.request.Request(endpoint + path, method=method,
                headers={'X-Hv2-Cluster-Token': cluster_token, 'Content-Type': 'application/json'},
                data=None if body is None else json.dumps(body).encode())
            with opener.open(request, timeout=60) as response:
                data = response.read()
                return json.loads(data) if data else None
        def execute(sandbox, command):
            result = api('POST', f'/sandboxes/{sandbox}/exec', {'cmd': command, 'timeout_secs': 20})
            if result['exit_code'] != 0:
                diagnostic = result.get('stderr', '')[:400]
                for value in (token, cluster_token, 'fixture-one', 'fixture-two'):
                    diagnostic = diagnostic.replace(value, '[redacted]')
                raise ValueError(f"owned guest command failed ({result['exit_code']}): {diagnostic}")
            return result['stdout']
        def request(sandbox, host='api.secret.example.test', expected=token):
            before = len(received)
            command = shlex.join(['curl', '--silent', '--show-error', '--fail', '--max-time', '12',
                '--noproxy', '*', '--cacert', '/etc/ssl/certs/ca-certificates.crt',
                '--resolve', f'{host}:{server.server_port}:{address}', '--user', f'user:{token}',
                '--header', f'X-Fixture: {token}', '--header', 'Content-Type: application/json',
                '--data-binary', json.dumps({'key': token}),
                f'https://{host}:{server.server_port}/fixture?key={token}'])
            require(execute(sandbox, command) == 'OK', 'owned HTTPS response differs')
            require(len(received) == before + 1, 'owned upstream request count differs')
            row = received[-1]
            headers = {k.lower(): v for k, v in row['headers'].items()}
            require(headers.get('x-fixture') == expected, 'header substitution differs')
            require(headers.get('authorization') == 'Basic ' + base64.b64encode(f'user:{expected}'.encode()).decode(), 'Basic substitution differs')
            require(urllib.parse.parse_qs(urllib.parse.urlsplit(row['path']).query) == {'key': [expected]}, 'query substitution differs')
            require(json.loads(row['body']) == {'key': expected}, 'JSON substitution differs')
            require(int(headers['content-length']) == len(row['body']), 'rewritten length differs')
        with (root / 'daemon.log').open('wb') as log:
            try:
                process = subprocess.Popen([str(args.daemon), '--network', '--port', str(api_port),
                    '--proxy-port', str(port()), '--snapshot-store', str(root / 'snapshots'),
                    '--tenant-reserved-cidr', address + '/32', '--egress-secrets-file', str(policy),
                    '--egress-upstream-ca', str(root / 'root.pem')],
                    env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd),
                             HV2_CLUSTER_TOKEN=cluster_token, RUST_LOG='info'), stdout=log, stderr=log)
                deadline = time.monotonic() + 30
                while True:
                    require(process.poll() is None, 'owned daemon exited during readiness')
                    try:
                        if api('GET', '/sandboxes') == [] and any('base' in row.get('aliases', []) for row in api('GET', '/templates')): break
                    except OSError:
                        pass
                    require(time.monotonic() < deadline, 'owned daemon readiness timeout'); time.sleep(.05)
                sandbox = api('POST', '/sandboxes', {'templateID': 'base', 'timeout': 300,
                              'network': {'allowOut': [address + '/32']}})['sandboxID']
                require('https' in execute(sandbox, 'curl --version'), 'guest client lacks HTTPS')
                ca = (root / 'root.pem').read_text()
                execute(sandbox, "printf '%s' " + shlex.quote(ca) + ' >> /etc/ssl/certs/ca-certificates.crt')
                request(sandbox); checks.append('Unscoped guest sends placeholders over verified owned HTTPS')
                def reload(value):
                    document = {'version': 1, 'sandboxes': [] if value is None else [{'sandbox_id': sandbox,
                        'bindings': [{'placeholder': token, 'value': value, 'hosts': ['api.secret.example.test']}]}]}
                    count = (root / 'daemon.log').read_text().count('secret policy reload completed')
                    policy.write_text(json.dumps(document)); process.send_signal(signal.SIGHUP)
                    deadline = time.monotonic() + 10
                    while (root / 'daemon.log').read_text().count('secret policy reload completed') <= count:
                        require(process.poll() is None and time.monotonic() < deadline, 'policy reload timeout'); time.sleep(.02)
                reload('fixture-one'); request(sandbox, expected='fixture-one')
                checks.append('Exact sandbox scope rewrites headers, Basic auth, query and JSON with correct framing')
                request(sandbox, host='other.secret.example.test')
                checks.append('Different authenticated hostname preserves placeholders')
                reload('fixture-two'); request(sandbox, expected='fixture-two')
                checks.append('SIGHUP rotation preserves placeholders and uses updated secret')
                forks = api('POST', f'/sandboxes/{sandbox}/fork', {'count': 1, 'timeout': 120})
                require(len(forks) == 1 and 'sandbox' in forks[0], 'owned fork failed')
                child = forks[0]['sandbox']['sandboxID']
                require(child != sandbox, 'fork reused parent sandbox ID')
                request(child)
                checks.append('Fork inherits network policy but not parent operator secret scope')
                api('DELETE', f'/sandboxes/{child}')
                api('POST', f'/sandboxes/{sandbox}/pause', {})
                api('POST', f'/sandboxes/{sandbox}/resume', {})
                request(sandbox, expected='fixture-two')
                checks.append('Pause/resume reattaches the exact sandbox scope')
                reload(None); request(sandbox)
                checks.append('SIGHUP scope removal revokes substitution')
                api('DELETE', f'/sandboxes/{sandbox}')
                require(api('GET', '/sandboxes') == [], 'owned guest cleanup failed')
            finally:
                if process is not None:
                    process.terminate()
                    try: process.wait(timeout=10)
                    except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
                server.shutdown(); server.server_close(); thread.join(timeout=5)
                require(not thread.is_alive(), 'owned HTTPS listener remains')
    require(hashes == {name: digest(getattr(args, name)) for name in hashes}, 'fixture input changed')
    report = {'checks': checks, 'input_sha256': hashes, 'upstream_requests': len(received),
              'processes_reaped': True, 'limits': [
              'No managed competitor measurement or performance claim', 'New TLS connection per curl invocation',
              'Concurrent lifecycle/reload races not exercised']}
    with args.output.open('x') as stream:
        json.dump(report, stream, indent=2); stream.write('\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
