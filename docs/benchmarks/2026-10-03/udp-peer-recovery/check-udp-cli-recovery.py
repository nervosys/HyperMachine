#!/usr/bin/env python3
"""Verify UDP upstream closure releases a capped CLI slot without restarting."""
import argparse
import hashlib
import json
import os
import select
import signal
import socket
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    cli = args.cli.resolve(strict=True)
    binary_hash = hashlib.sha256(cli.read_bytes()).hexdigest()
    seen = []
    closed = threading.Event()
    failures = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            try:
                assert self.headers.get('X-Api-Key') == 'owned-udp-key'
                assert self.headers.get('Upgrade') == 'hv2-udp/1'
                assert self.path == '/sandboxes/owned/ports/5353/udp'
                seen.append(self.client_address)
                self.send_response(101)
                self.send_header('Connection', 'upgrade')
                self.send_header('Upgrade', 'hv2-udp/1')
                self.end_headers()
                self.connection.settimeout(5)
                self.close_connection = True
                while True:
                    prefix = self.rfile.read(2)
                    if not prefix:
                        break
                    assert len(prefix) == 2
                    size = int.from_bytes(prefix, 'big')
                    assert size <= 65507
                    body = self.rfile.read(size)
                    assert len(body) == size
                    if body == b'close-owned-session':
                        self.connection.shutdown(socket.SHUT_RDWR)
                        closed.set()
                        return
                    self.wfile.write(prefix + body)
                    self.wfile.flush()
            except OSError:
                pass
            except Exception as error:
                failures.append(str(error))

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = False
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    process = None
    peers = []
    checks = []
    try:
        with (args.output / 'cli.log').open('xb') as error:
            process = subprocess.Popen([str(cli), 'sandbox', 'vm', '--endpoint',
                f'http://127.0.0.1:{server.server_port}', 'udp', 'owned', '--port', '5353', '--max-peers', '1'],
                env=dict(os.environ, HV2_API_KEY='owned-udp-key'), stdout=subprocess.PIPE, stderr=error)
            if not select.select([process.stdout], [], [], 5)[0]:
                raise TimeoutError('missing listener announcement')
            ready = json.loads(process.stdout.readline())
            host, port = ready['listen'].rsplit(':', 1)
            address = (host, int(port))
            for _ in range(2):
                peer = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                peer.bind(('127.0.0.1', 0))
                peer.settimeout(.25)
                peers.append(peer)

            def exchange(peer, message):
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    peer.sendto(message, address)
                    try:
                        if peer.recvfrom(65508)[0] != message:
                            raise ValueError('recovered peer bytes differ')
                        return
                    except socket.timeout:
                        if process.poll() is not None:
                            raise ValueError('CLI exited during recovery')
                raise TimeoutError('UDP session did not recover')

            def disconnect(peer):
                closed.clear()
                peer.sendto(b'close-owned-session', address)
                if not closed.wait(5):
                    raise TimeoutError('fixture did not close upstream')

            exchange(peers[0], b'first-peer\x00\xff')
            peers[1].sendto(b'capacity-refusal', address)
            try:
                peers[1].recvfrom(64)
                raise ValueError('peer limit exceeded')
            except socket.timeout:
                pass
            if len(seen) != 1:
                raise ValueError('extra capped peer opened a session')
            checks.append('Live peer cap enforced')
            disconnect(peers[0])
            exchange(peers[1], b'new-peer-after-closure\x00\xff')
            if len(seen) != 2:
                raise ValueError('new peer did not obtain a fresh session')
            checks.append('Upstream EOF releases capacity for a new source peer')
            disconnect(peers[1])
            exchange(peers[1], b'same-peer-after-closure\x00\xff')
            if len(seen) != 3:
                raise ValueError('existing peer did not reconnect')
            checks.append('Existing source peer reconnects after upstream EOF')
            process.send_signal(signal.SIGINT)
            if process.wait(timeout=5) != 0:
                raise ValueError('CLI interrupt failed')
            checks.append('CLI exits cleanly after recovery')
    finally:
        for peer in peers:
            peer.close()
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=5)
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        if thread.is_alive():
            raise ValueError('owned server not reaped')
    if failures:
        raise ValueError('owned server checks failed: ' + repr(failures))
    if hashlib.sha256(cli.read_bytes()).hexdigest() != binary_hash:
        raise ValueError('CLI binary changed during fixture')
    report = {'cli_sha256': binary_hash, 'checks': checks, 'sessions': len(seen),
              'max_peers': 1, 'cli_reaped': process.poll() == 0, 'server_reaped': True}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
