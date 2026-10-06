#!/usr/bin/env python3
"""Verify framed UDP through an owned authenticated KVM sandbox daemon."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('daemon', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (args.daemon, args.kernel, args.initrd)}
    def port():
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            return sock.getsockname()[1]
    api_port = port()
    token = secrets.token_hex(32)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def api(method, path, body=None):
        request = urllib.request.Request(f'http://127.0.0.1:{api_port}' + path, method=method,
            headers={'X-Hv2-Cluster-Token': token, 'Content-Type': 'application/json'},
            data=None if body is None else json.dumps(body).encode())
        with opener.open(request, timeout=30) as response:
            data = response.read()
            return json.loads(data) if data else None
    def tunnel(id, credential=token):
        sock = socket.create_connection(('127.0.0.1', api_port), timeout=5)
        sock.sendall((f'GET /sandboxes/{id}/ports/5353/udp HTTP/1.1\r\nHost: localhost\r\n'
            f'Connection: Upgrade\r\nUpgrade: hv2-udp/1\r\nX-Hv2-Cluster-Token: {credential}\r\n\r\n').encode())
        headers = bytearray()
        while not headers.endswith(b'\r\n\r\n'):
            byte = sock.recv(1)
            if not byte or len(headers) > 8192:
                sock.close()
                raise ValueError('invalid upgrade response')
            headers.extend(byte)
        status = int(headers.split(b' ', 2)[1])
        return sock, status
    def exact(sock, count):
        out = bytearray()
        while len(out) < count:
            data = sock.recv(count - len(out))
            if not data: raise ValueError('truncated datagram')
            out.extend(data)
        return bytes(out)
    def roundtrip(sock, payload):
        sock.sendall(len(payload).to_bytes(2, 'big') + payload)
        size = int.from_bytes(exact(sock, 2), 'big')
        if exact(sock, size) != payload: raise ValueError('UDP payload differs')
    def closed(sock):
        try:
            if sock.recv(1) != b'': raise ValueError('lifecycle left data session open')
        except ConnectionResetError:
            pass
    process = None
    streams = []
    sandbox = None
    checks = []
    with tempfile.TemporaryDirectory(prefix='hm-udp-kvm-') as temporary:
        root = Path(temporary)
        with (args.output / 'daemon.log').open('wb') as log:
            try:
                process = subprocess.Popen([str(args.daemon), '--port', str(api_port), '--proxy-port', str(port()),
                    '--snapshot-store', str(root / 'snapshots')], stdout=log, stderr=log,
                    env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd),
                             HV2_CLUSTER_TOKEN=token, RUST_LOG='info'))
                deadline = time.monotonic() + 30
                while True:
                    if process.poll() is not None: raise ValueError('daemon exited')
                    try:
                        if api('GET', '/sandboxes') == [] and any('base' in row.get('aliases', []) for row in api('GET', '/templates')): break
                    except OSError: pass
                    if time.monotonic() > deadline: raise TimeoutError('daemon readiness')
                    time.sleep(.05)
                sandbox = api('POST', '/sandboxes', {'templateID': 'base', 'timeout': 300})['sandboxID']
                result = api('POST', f'/sandboxes/{sandbox}/exec',
                    {'cmd': '/bin/hm-udp-echo >/tmp/udp-echo.log 2>&1 &', 'timeout_secs': 5})
                if result['exit_code'] != 0: raise ValueError('echo service launch failed')
                time.sleep(.1)
                refused, status = tunnel(sandbox, 'wrong-owned-token')
                refused.close()
                if status != 401: raise ValueError('wrong cluster credential accepted')
                checks.append('Wrong cluster credential refused')
                stream, status = tunnel(sandbox)
                streams.append(stream)
                if status != 101: raise ValueError(f'guest UDP setup returned {status}')
                for payload in (b'', b'\x00\xff\r\n', bytes(n % 251 for n in range(65507))): roundtrip(stream, payload)
                checks.append('Empty binary and maximum-size IPv4 datagrams verified through real KVM')
                api('POST', f'/sandboxes/{sandbox}/pause', {})
                closed(stream)
                checks.append('Pause closes existing UDP session')
                api('POST', f'/sandboxes/{sandbox}/resume', {})
                resumed, status = tunnel(sandbox)
                streams.append(resumed)
                if status != 101: raise ValueError('resumed UDP setup failed')
                roundtrip(resumed, b'after-resume\x00\xff')
                checks.append('Resumed guest accepts a new UDP session')
                api('DELETE', f'/sandboxes/{sandbox}')
                sandbox = None
                closed(resumed)
                if api('GET', '/sandboxes') != []: raise ValueError('guest inventory not empty')
                checks.append('Delete closes UDP session and clears guest inventory')
            finally:
                for stream in streams: stream.close()
                if process is not None and process.poll() is None:
                    if sandbox:
                        try: api('DELETE', f'/sandboxes/{sandbox}')
                        except OSError: pass
                    process.terminate()
                    try: process.wait(timeout=10)
                    except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
    for path, expected in inputs.items():
        if hashlib.sha256(Path(path).read_bytes()).hexdigest() != expected: raise ValueError('input changed')
    report = {'inputs_sha256': inputs, 'checks': checks, 'daemon_reaped': True, 'guests_remaining': 0,
        'scope': 'Owned authenticated standalone daemon, actual KVM/vsock/guest UDP; no CLI/control plane/TLS.'}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
