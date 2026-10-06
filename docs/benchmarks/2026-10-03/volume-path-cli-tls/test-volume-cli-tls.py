#!/usr/bin/env python3
"""Verify shipped hm volume upload/download over owned HTTP and HTTPS."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import ssl
import subprocess
import tempfile
import threading
import time
import unittest

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--cli', type=Path, required=True)
options, remaining = parser.parse_known_args()


class VolumeCliTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='hm-volume-cli-tls-')
        self.root = Path(self.temporary.name)
        self.payload = bytes(range(256)) * 4096
        self.source = self.root / 'source.bin'
        self.source.write_bytes(self.payload)
        self.destination = self.root / 'download.bin'
        self.token = secrets.token_hex(32)
        self.requests = []
        self.mode = 'ok'
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_PUT(self):
                self.transfer(True)

            def do_GET(self):
                self.transfer(False)

            def do_POST(self):
                self.transfer(False)

            def transfer(self, upload):
                body = self.rfile.read(int(self.headers['Content-Length'])) if upload else b''
                fixture.requests.append({'upload': upload,
                    'method': self.command, 'path': self.path,
                    'authorized': self.headers.get('Authorization') == 'Bearer ' + fixture.token,
                    'api_key_present': 'X-Api-Key' in self.headers,
                    'digest': hashlib.sha256(body).hexdigest() if upload else None})
                if fixture.mode == 'redirect':
                    self.send_response(307)
                    self.send_header('Location', '/refused-redirect')
                    self.send_header('Content-Length', '0')
                    self.end_headers()
                    return
                response = json.dumps({'size': len(body)}).encode() if upload else fixture.payload
                if '/dir?' in self.path or '/path?' in self.path:
                    entry = {'name': 'nested', 'path': '/nested', 'type': 'directory'}
                    response = json.dumps([entry] if self.command == 'GET' and '/dir?' in self.path else entry).encode()
                declared = len(response)
                if fixture.mode == 'oversized': declared = 4 * 1024**3 + 1
                if fixture.mode == 'truncated': declared += 100
                self.send_response(201 if upload or self.command == 'POST' else 200)
                self.send_header('Content-Length', str(declared))
                self.end_headers()
                try:
                    if fixture.mode == 'slow':
                        for byte in response:
                            self.wfile.write(bytes([byte])); self.wfile.flush(); time.sleep(.1)
                    elif fixture.mode != 'oversized':
                        self.wfile.write(response)
                except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
                    pass

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.server.daemon_threads = False
        self.thread = None

    def start(self, tls=False):
        if tls:
            subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ed25519', '-nodes', '-days', '1',
                '-keyout', str(self.root / 'key.pem'), '-out', str(self.root / 'cert.pem'),
                '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost'],
                capture_output=True, check=True, timeout=20)
            subprocess.run(['openssl', 'req', '-new', '-newkey', 'ed25519', '-nodes',
                '-keyout', str(self.root / 'server.key'), '-out', str(self.root / 'server.csr'),
                '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost',
                '-addext', 'basicConstraints=critical,CA:FALSE'], capture_output=True, check=True, timeout=20)
            subprocess.run(['openssl', 'x509', '-req', '-in', str(self.root / 'server.csr'),
                '-CA', str(self.root / 'cert.pem'), '-CAkey', str(self.root / 'key.pem'),
                '-CAcreateserial', '-copy_extensions', 'copy', '-days', '1',
                '-out', str(self.root / 'server.pem')], capture_output=True, check=True, timeout=20)
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(self.root / 'server.pem', self.root / 'server.key')
            self.server.socket = context.wrap_socket(self.server.socket, server_side=True)
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()
        return f'{"https" if tls else "http"}://localhost:{self.server.server_port}'

    def invoke(self, endpoint, upload, trusted=False, timeout=5):
        command = [str(options.cli), 'sandbox', 'vm', '--endpoint', endpoint, '--request-timeout', str(timeout)]
        if trusted: command += ['--api-ca-cert', str(self.root / 'cert.pem')]
        command += ['volume', 'upload' if upload else 'download', 'vol-owned',
                    str(self.source if upload else self.destination), '--path', '/fixture.bin']
        result = subprocess.run(command, env=dict(os.environ, HV2_VOLUME_TOKEN=self.token,
            HV2_API_KEY='invalid\nplatform-key-must-not-be-used'), capture_output=True, timeout=8)
        self.assertNotIn(self.token.encode(), result.stdout + result.stderr)
        return result

    def invoke_path(self, endpoint, operation, trusted=False):
        command = [str(options.cli), 'sandbox', 'vm', '--endpoint', endpoint]
        if trusted: command += ['--api-ca-cert', str(self.root / 'cert.pem')]
        command += ['volume', operation, 'vol-owned', '--path', '/nested']
        result = subprocess.run(command, env=dict(os.environ, HV2_VOLUME_TOKEN=self.token,
            HV2_API_KEY='invalid\nplatform-key-must-not-be-used'), capture_output=True, timeout=8)
        self.assertNotIn(self.token.encode(), result.stdout + result.stderr)
        return result

    def test_path_commands_validate_tls_and_use_only_content_token(self):
        endpoint = self.start(tls=True)
        for operation in ('ls', 'stat', 'mkdir'):
            self.assertNotEqual(self.invoke_path(endpoint, operation).returncode, 0)
            self.assertNotEqual(self.invoke_path(endpoint.replace('localhost', '127.0.0.1'), operation, trusted=True).returncode, 0)
        self.assertEqual(self.requests, [])
        for operation in ('ls', 'stat', 'mkdir'):
            result = self.invoke_path(endpoint, operation, trusted=True)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            value = json.loads(result.stdout)
            self.assertEqual((value[0] if operation == 'ls' else value)['type'], 'directory')
        self.assertEqual([row['method'] for row in self.requests], ['GET', 'GET', 'POST'])
        self.assertTrue(all(row['authorized'] and not row['api_key_present'] for row in self.requests))

    def tearDown(self):
        if self.thread: self.server.shutdown()
        self.server.server_close()
        if self.thread:
            self.thread.join(timeout=5)
            self.assertFalse(self.thread.is_alive())
        self.temporary.cleanup()

    def test_trusted_https_roundtrip(self):
        endpoint = self.start(tls=True)
        uploaded = self.invoke(endpoint, True, trusted=True)
        self.assertEqual(uploaded.returncode, 0, uploaded.stderr.decode(errors='replace'))
        self.assertEqual(self.invoke(endpoint, False, trusted=True).returncode, 0)
        self.assertEqual(self.destination.read_bytes(), self.payload)
        self.assertEqual(self.requests[0]['digest'], hashlib.sha256(self.payload).hexdigest())
        self.assertTrue(all(row['authorized'] and not row['api_key_present'] for row in self.requests))

    def test_untrusted_and_wrong_hostname_send_no_http_payload(self):
        endpoint = self.start(tls=True)
        for upload in (True, False):
            self.assertNotEqual(self.invoke(endpoint, upload).returncode, 0)
            self.assertNotEqual(self.invoke(endpoint.replace('localhost', '127.0.0.1'), upload, trusted=True).returncode, 0)
        self.assertEqual(self.requests, [])
        self.assertFalse(self.destination.exists())

    def test_redirect_is_refused_for_both_commands(self):
        self.mode = 'redirect'
        endpoint = self.start()
        for upload in (True, False): self.assertNotEqual(self.invoke(endpoint, upload).returncode, 0)
        self.assertEqual(len(self.requests), 2)
        self.assertFalse(self.destination.exists())

    def test_slow_response_deadline_and_download_cleanup(self):
        self.mode = 'slow'
        endpoint = self.start()
        for upload in (True, False):
            started = time.monotonic()
            self.assertNotEqual(self.invoke(endpoint, upload, timeout=1).returncode, 0)
            self.assertLess(time.monotonic() - started, 3)
        self.assertFalse(self.destination.exists())
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ['source.bin'])

    def test_truncated_and_oversized_downloads_leave_no_file(self):
        endpoint = self.start()
        for mode in ('truncated', 'oversized'):
            self.mode = mode
            self.assertNotEqual(self.invoke(endpoint, False).returncode, 0)
            self.assertFalse(self.destination.exists())
            self.assertEqual(sorted(p.name for p in self.root.iterdir()), ['source.bin'])


if __name__ == '__main__':
    unittest.main(argv=[__file__, *remaining])
