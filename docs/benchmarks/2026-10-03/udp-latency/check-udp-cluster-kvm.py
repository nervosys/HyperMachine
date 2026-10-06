#!/usr/bin/env python3
"""Verify framed UDP through an owned shipped CLI/control-plane/Redis/KVM stack."""
import argparse
import hashlib
import json
import math
import statistics
import os
from pathlib import Path
import secrets
import select
import signal
import socket
import ssl
import subprocess
import tempfile
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('daemon', 'control_plane', 'cli', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name.replace('_', '-'), type=Path, required=True)
    parser.add_argument('--tls', action='store_true')
    parser.add_argument('--latency-samples', type=int, default=0)
    args = parser.parse_args()
    if args.latency_samples != 0 and not 100 <= args.latency_samples <= 10000:
        parser.error('latency samples must be zero or 100–10000')
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (args.daemon, args.control_plane, args.cli, args.kernel, args.initrd)}
    def port():
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            return sock.getsockname()[1]
    api_port = port()
    node_port = port()
    node_proxy = port()
    redis_port = port()
    store_url = f'redis://127.0.0.1:{redis_port}'
    namespace = secrets.token_hex(8)
    cluster_token = secrets.token_hex(32)
    token = secrets.token_hex(32)
    base = f'{"https" if args.tls else "http"}://localhost:{api_port}'
    context = None
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def api(method, path, body=None):
        request = urllib.request.Request(base + path, method=method,
            headers={'X-Api-Key': token, 'Content-Type': 'application/json'},
            data=None if body is None else json.dumps(body).encode())
        with opener.open(request, timeout=30) as response:
            data = response.read()
            return json.loads(data) if data else None
    def tunnel(id, credential=token):
        sock = socket.create_connection(('127.0.0.1', api_port), timeout=5)
        if args.tls: sock = context.wrap_socket(sock, server_hostname='localhost')
        sock.sendall((f'GET /sandboxes/{id}/ports/5353/udp HTTP/1.1\r\nHost: localhost\r\n'
            f'Connection: Upgrade\r\nUpgrade: hv2-udp/1\r\nX-Api-Key: {credential}\r\n\r\n').encode())
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
    services = []
    cli_process = None
    streams = []
    sandbox = None
    checks = []
    measurements = []
    def measure(label, transfer):
        payload = bytes(range(64))
        for _ in range(10): transfer(payload)
        samples = []
        for _ in range(args.latency_samples):
            start = time.perf_counter_ns()
            transfer(payload)
            samples.append((time.perf_counter_ns() - start) / 1e6)
        ordered = sorted(samples)
        measurements.append({'path': label, 'payload_bytes': 64, 'warmups': 10, 'samples_ms': samples,
            'p50_ms': statistics.median(samples), 'p95_ms': ordered[math.ceil(len(ordered)*.95)-1],
            'p99_ms': ordered[math.ceil(len(ordered)*.99)-1], 'concurrency': 1})
    with tempfile.TemporaryDirectory(prefix='hm-udp-kvm-') as temporary:
        root = Path(temporary)
        tls_arguments = []
        if args.tls:
            subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ed25519', '-nodes', '-days', '1',
                '-keyout', str(root / 'ca.key'), '-out', str(root / 'ca.pem'), '-subj', '/CN=owned-udp-root',
                '-addext', 'keyUsage=critical,keyCertSign,cRLSign'],
                capture_output=True, check=True, timeout=20)
            subprocess.run(['openssl', 'req', '-new', '-newkey', 'ed25519', '-nodes',
                '-keyout', str(root / 'leaf.key'), '-out', str(root / 'leaf.csr'), '-subj', '/CN=localhost',
                '-addext', 'subjectAltName=DNS:localhost', '-addext', 'basicConstraints=critical,CA:FALSE'],
                capture_output=True, check=True, timeout=20)
            subprocess.run(['openssl', 'x509', '-req', '-in', str(root / 'leaf.csr'), '-CA', str(root / 'ca.pem'),
                '-CAkey', str(root / 'ca.key'), '-CAcreateserial', '-copy_extensions', 'copy', '-days', '1',
                '-out', str(root / 'leaf.pem')], capture_output=True, check=True, timeout=20)
            context = ssl.create_default_context(cafile=root / 'ca.pem')
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=context))
            tls_arguments = ['--api-tls-cert', str(root / 'leaf.pem'), '--api-tls-key', str(root / 'leaf.key')]

        with (args.output / 'daemon.log').open('wb') as log:
            try:
                redis = subprocess.Popen(['redis-server', '--bind', '127.0.0.1', '--port', str(redis_port), '--save', '', '--appendonly', 'no', '--dir', str(root)], stdout=log, stderr=log)
                services.append(redis)
                ready = time.monotonic() + 10
                while True:
                    try:
                        with socket.create_connection(('127.0.0.1', redis_port), timeout=.2): break
                    except OSError:
                        if redis.poll() is not None or time.monotonic() > ready: raise TimeoutError('owned Redis readiness')
                        time.sleep(.02)
                control = subprocess.Popen([str(args.control_plane), '--store', store_url, '--namespace', namespace, '--port', str(api_port), '--proxy-port', str(port())] + tls_arguments, stdout=log, stderr=log, env=dict(os.environ, HV2_API_KEY=token, HV2_CLUSTER_TOKEN=cluster_token))
                services.append(control)
                process = subprocess.Popen([str(args.daemon), '--port', str(node_port), '--proxy-port', str(node_proxy),
                    '--cluster-store', store_url, '--cluster-namespace', namespace, '--node-id', 'owned-udp-node',
                    '--advertise-api', f'http://127.0.0.1:{node_port}', '--advertise-proxy', f'127.0.0.1:{node_proxy}',
                    '--snapshot-store', str(root / 'snapshots')], stdout=log, stderr=log,
                    env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd),
                             HV2_CLUSTER_TOKEN=cluster_token, RUST_LOG='info'))
                deadline = time.monotonic() + 30
                last_error = None
                while True:
                    if process.poll() is not None: raise ValueError('daemon exited')
                    try:
                        if api('GET', '/sandboxes') == [] and any('base' in row.get('aliases', []) for row in api('GET', '/templates')): break
                    except OSError as error: last_error = str(error)
                    if time.monotonic() > deadline: raise TimeoutError(f'daemon readiness: {last_error}')
                    time.sleep(.05)
                if args.tls:
                    for endpoint, trusted in [(base, False), (base.replace('localhost', '127.0.0.1'), True)]:
                        command = [str(args.cli), 'sandbox', 'vm', '--endpoint', endpoint]
                        if trusted: command += ['--api-ca-cert', str(root / 'ca.pem')]
                        command += ['list']
                        refused = subprocess.run(command, env=dict(os.environ, HV2_API_KEY=token), capture_output=True, timeout=8)
                        if refused.returncode == 0: raise ValueError('CLI accepted untrusted or wrong-host TLS')
                        if token.encode() in refused.stdout + refused.stderr: raise ValueError('CLI exposed fixture API key')
                    checks.append('CLI refuses untrusted control certificate and trusted wrong hostname')
                sandbox = api('POST', '/sandboxes', {'templateID': 'base', 'timeout': 300})['sandboxID']
                result = api('POST', f'/sandboxes/{sandbox}/exec',
                    {'cmd': '/bin/hm-udp-echo >/tmp/udp-echo.log 2>&1 &', 'timeout_secs': 5})
                if result['exit_code'] != 0: raise ValueError('echo service launch failed')
                time.sleep(.1)
                refused, status = tunnel(sandbox, 'wrong-owned-token')
                refused.close()
                if status != 401: raise ValueError('wrong cluster credential accepted')
                checks.append('Wrong control-plane API key refused')
                stream, status = tunnel(sandbox)
                streams.append(stream)
                if status != 101: raise ValueError(f'guest UDP setup returned {status}')
                for payload in (b'', b'\x00\xff\r\n', bytes(n % 251 for n in range(65507))): roundtrip(stream, payload)
                checks.append('Empty binary and maximum-size IPv4 datagrams verified through real KVM')
                if args.latency_samples: measure('upgraded_control_api', lambda payload: roundtrip(stream, payload))
                malformed, status = tunnel(sandbox)
                streams.append(malformed)
                if status != 101: raise ValueError('malformed-session setup failed')
                malformed.sendall(bytes([255, 255]))
                closed(malformed)
                fresh, status = tunnel(sandbox)
                streams.append(fresh)
                if status != 101: raise ValueError('post-refusal session setup failed')
                roundtrip(fresh, b'after-refusal\x00\xff')
                fresh.close()
                checks.append('Oversized frame closes its real guest session without preventing a fresh valid session')

                cli_process = subprocess.Popen([str(args.cli), 'sandbox', 'vm', '--endpoint', base] + (['--api-ca-cert', str(root / 'ca.pem')] if args.tls else []) + [
                    'udp', sandbox, '--port', '5353', '--max-peers', '2'], stdout=subprocess.PIPE, stderr=log,
                    env=dict(os.environ, HV2_API_KEY=token))
                if not select.select([cli_process.stdout], [], [], 5)[0]: raise TimeoutError('CLI readiness')
                announced = json.loads(cli_process.stdout.readline())
                host, local_port = announced['listen'].rsplit(':', 1)
                peers = [socket.socket(socket.AF_INET, socket.SOCK_DGRAM) for _ in range(2)]
                try:
                    for peer in peers:
                        peer.bind(('127.0.0.1', 0)); peer.settimeout(5)
                    for size in (0, 4, 65507):
                        messages = [bytes([index + 1]) * size for index in range(2)]
                        for peer, payload in zip(peers, messages): peer.sendto(payload, (host, int(local_port)))
                        for peer, payload in zip(peers, messages):
                            if peer.recvfrom(65508)[0] != payload: raise ValueError('real CLI peer payload differs')
                    if args.latency_samples:
                        def cli_transfer(payload):
                            peers[0].sendto(payload, (host, int(local_port)))
                            if peers[0].recvfrom(65508)[0] != payload: raise ValueError('timed CLI bytes differ')
                        measure('cli_loopback_control_api', cli_transfer)

                finally:
                    for peer in peers: peer.close()
                cli_process.send_signal(signal.SIGINT)
                if cli_process.wait(timeout=5) != 0: raise ValueError('CLI interrupt failed')
                checks.append('Two CLI peers preserve empty binary and maximum-size datagrams through real control plane Redis node and KVM')

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
                if cli_process is not None and cli_process.poll() is None:
                    cli_process.kill(); cli_process.wait(timeout=5)
                if process is not None and process.poll() is None:
                    if sandbox:
                        try: api('DELETE', f'/sandboxes/{sandbox}')
                        except OSError: pass
                    process.terminate()
                    try: process.wait(timeout=10)
                    except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
                for service in reversed(services):
                    if service.poll() is None:
                        service.terminate()
                        try: service.wait(timeout=5)
                        except subprocess.TimeoutExpired: service.kill(); service.wait(timeout=5)
    for path, expected in inputs.items():
        if hashlib.sha256(Path(path).read_bytes()).hexdigest() != expected: raise ValueError('input changed')
    report = {'inputs_sha256': inputs, 'checks': checks, 'daemon_reaped': True, 'guests_remaining': 0,
        'control_and_redis_reaped': True, 'cli_reaped': True,
        'control_api_tls': args.tls, 'latency_measurements': measurements,
        'scope': 'Owned shipped CLI/control-plane/Redis/daemon/KVM UDP; optional verified API TLS, node hop HTTP, no mTLS.'}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
