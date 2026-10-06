#!/usr/bin/env python3
"""Exercise the volume upload helper against owned HTTP and HTTPS fixtures."""
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.parse

CLIENT = Path(__file__).with_name('upload-volume-file.py')


class UploadTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='hm-upload-client-test-')
        self.root = Path(self.temporary.name)
        self.source = self.root / 'source.bin'
        self.payload = bytes(range(256)) * 4096
        self.source.write_bytes(self.payload)
        self.token = secrets.token_hex(32)
        self.requests = []
        self.mode = 'ok'
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_PUT(self):
                body = self.rfile.read(int(self.headers['Content-Length']))
                fixture.requests.append({'path': self.path, 'digest': hashlib.sha256(body).hexdigest(),
                    'authorized': self.headers.get('Authorization') == 'Bearer ' + fixture.token,
                    'api_key_present': 'X-Api-Key' in self.headers})
                if fixture.mode == 'redirect':
                    self.send_response(307)
                    self.send_header('Location', '/credential-destination')
                    self.end_headers()
                    return
                if fixture.mode == 'stall':
                    time.sleep(.5)
                value = {'size': len(body) + (1 if fixture.mode == 'wrong-size' else 0)}
                encoded = json.dumps(value).encode()
                self.send_response(201)
                self.send_header('Content-Length', str(len(encoded)))
                self.end_headers()
                try:
                    self.wfile.write(encoded)
                except (BrokenPipeError, ConnectionResetError):
                    pass

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.server.daemon_threads = False
        self.thread = None

    def start(self, tls=False):
        if tls:
            command = ['openssl', 'req', '-x509', '-newkey', 'ed25519', '-nodes', '-days', '1',
                '-keyout', str(self.root / 'key.pem'), '-out', str(self.root / 'cert.pem'),
                '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost']
            subprocess.run(command, capture_output=True, check=True, timeout=20)
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(self.root / 'cert.pem', self.root / 'key.pem')
            self.server.socket = context.wrap_socket(self.server.socket, server_side=True)
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()
        return f'{"https" if tls else "http"}://localhost:{self.server.server_port}'

    def run_client(self, endpoint, *extra, token=True):
        environment = dict(os.environ)
        environment.pop('HV2_VOLUME_TOKEN', None)
        environment['HV2_API_KEY'] = 'owned-platform-key-must-not-be-forwarded'
        if token:
            environment['HV2_VOLUME_TOKEN'] = self.token
        result = subprocess.run([sys.executable, str(CLIENT), '--endpoint', endpoint,
            '--volume-id', 'vol-owned', '--path', '/nested/a b&c.bin', '--source', str(self.source), *extra],
            env=environment, capture_output=True, timeout=5)
        self.assertNotIn(self.token.encode(), result.stdout + result.stderr)
        return result

    def tearDown(self):
        if self.thread:
            self.server.shutdown()
        self.server.server_close()
        if self.thread:
            self.thread.join(timeout=5)
            self.assertFalse(self.thread.is_alive())
        self.temporary.cleanup()

    def test_trusted_https_binary_and_query(self):
        result = self.run_client(self.start(tls=True), '--ca-cert', str(self.root / 'cert.pem'), '--force')
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout)['size'], len(self.payload))
        self.assertEqual(len(self.requests), 1)
        row = self.requests[0]
        self.assertEqual(row['digest'], hashlib.sha256(self.payload).hexdigest())
        self.assertTrue(row['authorized'])
        self.assertFalse(row['api_key_present'])
        self.assertEqual(urllib.parse.parse_qs(urllib.parse.urlsplit(row['path']).query),
            {'path': ['/nested/a b&c.bin'], 'atomic': ['true'], 'force': ['true']})

    def test_untrusted_certificate_refused(self):
        self.assertNotEqual(self.run_client(self.start(tls=True)).returncode, 0)
        self.assertEqual(self.requests, [])

    def test_wrong_hostname_refused(self):
        endpoint = self.start(tls=True).replace('localhost', '127.0.0.1')
        self.assertNotEqual(self.run_client(endpoint, '--ca-cert', str(self.root / 'cert.pem')).returncode, 0)
        self.assertEqual(self.requests, [])

    def test_redirect_is_not_followed(self):
        self.mode = 'redirect'
        self.assertNotEqual(self.run_client(self.start()).returncode, 0)
        self.assertEqual(len(self.requests), 1)

    def test_invalid_inputs_send_no_request(self):
        endpoint = self.start()
        self.assertNotEqual(self.run_client(endpoint, token=False).returncode, 0)
        self.assertNotEqual(self.run_client('http://192.0.2.1').returncode, 0)
        with self.source.open('wb') as source:
            source.truncate(4 * 1024**3 + 1)
        self.assertNotEqual(self.run_client(endpoint).returncode, 0)
        self.assertEqual(self.requests, [])

    def test_empty_in_place_file(self):
        self.source.write_bytes(b'')
        result = self.run_client(self.start(), '--in-place')
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout)['size'], 0)
        self.assertIn('atomic=false', self.requests[0]['path'])

    def test_timeout_and_wrong_response_size_refused(self):
        endpoint = self.start()
        self.mode = 'stall'
        self.assertNotEqual(self.run_client(endpoint, '--timeout', '0.15').returncode, 0)
        time.sleep(.6)
        self.mode = 'wrong-size'
        self.assertNotEqual(self.run_client(endpoint).returncode, 0)


if __name__ == '__main__':
    unittest.main()
