#!/usr/bin/env python3
"""Verify shipped volume CLI streaming against an owned bounded-memory HTTP fixture."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--mib', type=int, default=64)
    args = parser.parse_args()
    if not 1 <= args.mib <= 4096:
        parser.error('mib must be between 1 and 4096')
    args.output.mkdir(parents=True, exist_ok=False)
    cli = args.cli.resolve()
    initial_hash = hashlib.sha256(cli.read_bytes()).hexdigest()
    payload_size = args.mib * 1024**2
    block = bytes(range(256)) * 512
    expected = hashlib.sha256()
    for _ in range(payload_size // len(block)):
        expected.update(block)
    expected_hash = expected.hexdigest()
    token = secrets.token_hex(32)
    observations = []

    class Handler(BaseHTTPRequestHandler):
        def setup(self):
            super().setup()
            self.connection.settimeout(10)

        def log_message(self, *args):
            pass

        def authorized(self):
            if self.headers.get('Authorization') != 'Bearer ' + token or 'X-Api-Key' in self.headers:
                self.send_error(401)
                return False
            return True

        def do_PUT(self):
            if not self.authorized():
                return
            remaining = int(self.headers['Content-Length'])
            total = remaining
            digest = hashlib.sha256()
            while remaining:
                chunk = self.rfile.read(min(len(block), remaining))
                if not chunk:
                    raise RuntimeError('truncated upload')
                digest.update(chunk)
                remaining -= len(chunk)
            observations.append({'direction': 'upload', 'bytes': total, 'sha256': digest.hexdigest()})
            body = json.dumps({'size': total}).encode()
            self.send_response(201)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if not self.authorized():
                return
            self.send_response(200)
            self.send_header('Content-Length', str(payload_size))
            self.end_headers()
            for _ in range(payload_size // len(block)):
                self.wfile.write(block)
            observations.append({'direction': 'download', 'bytes': payload_size})

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = False
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    rows = []
    try:
        with tempfile.TemporaryDirectory(prefix='hm-volume-streaming-') as temporary:
            root = Path(temporary)
            source = root / 'source.bin'
            destination = root / 'download.bin'
            with source.open('wb') as output:
                for _ in range(payload_size // len(block)):
                    output.write(block)
            environment = dict(os.environ, HV2_VOLUME_TOKEN=token, HV2_API_KEY='ignored-platform-key')
            for direction, path in [('upload', source), ('download', destination)]:
                command = [str(cli), 'sandbox', 'vm', '--endpoint', f'http://127.0.0.1:{server.server_port}',
                    '--request-timeout', '120', 'volume', direction, 'vol-owned', str(path), '--path', '/large.bin']
                # Spool stdout/stderr, never accumulate transfer-sized output.
                with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
                    started = time.monotonic()
                    process = subprocess.Popen(command, env=environment, stdout=stdout, stderr=stderr)
                    samples = []
                    peak = 0
                    try:
                        while process.poll() is None:
                            if time.monotonic() - started > 130:
                                raise TimeoutError('CLI exceeded owned fixture timeout')
                            try:
                                status = Path(f'/proc/{process.pid}/status').read_text()
                                fields = dict(line.split(':', 1) for line in status.splitlines() if ':' in line)
                                rss = int(fields.get('VmRSS', '0 kB').split()[0])
                                peak = max(peak, int(fields.get('VmHWM', '0 kB').split()[0]))
                                samples.append(rss)
                            except FileNotFoundError:
                                pass
                            time.sleep(.002)
                        elapsed = time.monotonic() - started
                        stdout.seek(0)
                        stderr.seek(0)
                        output = stdout.read(65537)
                        error = stderr.read(65537)
                        if process.returncode != 0:
                            raise RuntimeError(f'CLI {direction} failed: {error.decode(errors="replace")}')
                        if token.encode() in output + error:
                            raise RuntimeError('credential appeared in CLI output')
                        result = json.loads(output)
                        if result['size'] != payload_size or not samples:
                            raise RuntimeError('incorrect byte count or missing memory samples')
                        rows.append({'direction': direction, 'bytes': payload_size, 'elapsed_seconds': elapsed,
                            'mib_per_second': args.mib / elapsed, 'rss_samples': len(samples),
                            'sampled_peak_rss_kib': max(samples), 'observed_peak_hwm_kib': peak})
                    finally:
                        if process.poll() is None:
                            process.kill()
                        process.wait(timeout=5)
            actual = hashlib.sha256()
            with destination.open('rb') as downloaded:
                while chunk := downloaded.read(len(block)):
                    actual.update(chunk)
            if actual.hexdigest() != expected_hash:
                raise RuntimeError('download byte verification failed')
            if observations != [{'direction': 'upload', 'bytes': payload_size, 'sha256': expected_hash},
                                {'direction': 'download', 'bytes': payload_size}]:
                raise RuntimeError('server transfer verification failed')
            if sorted(p.name for p in root.iterdir()) != ['download.bin', 'source.bin']:
                raise RuntimeError('download left unexpected staging file')
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        if thread.is_alive():
            raise RuntimeError('fixture server did not stop')
    if hashlib.sha256(cli.read_bytes()).hexdigest() != initial_hash:
        raise RuntimeError('CLI binary changed during verification')
    report = {'cli_sha256': initial_hash, 'payload_sha256': expected_hash, 'transfers': rows,
        'server_reaped': True, 'source_and_destination_removed': True,
        'scope': 'Owned loopback HTTP protocol fixture, development binary, no sandbox/control plane/TLS/competitor.'}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
