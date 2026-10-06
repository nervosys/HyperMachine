#!/usr/bin/env python3
"""Verify framed UDP through an owned shipped CLI/control-plane/Redis/KVM stack."""
import argparse
from concurrent.futures import ThreadPoolExecutor
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
import threading
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('daemon', 'control_plane', 'cli', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name.replace('_', '-'), type=Path, required=True)
    parser.add_argument('--native-gateway', type=Path)
    parser.add_argument('--tls', action='store_true')
    parser.add_argument('--mtls', action='store_true')
    parser.add_argument('--idle-check', action='store_true')
    parser.add_argument('--probe-ipv6', action='store_true')
    parser.add_argument('--guest-ipv6', action='store_true')
    parser.add_argument('--local-ipv6', action='store_true')
    parser.add_argument('--concurrent-samples', type=int, default=0)
    parser.add_argument('--concurrent-payload-bytes', type=int, default=64)
    parser.add_argument('--peer-count', type=int, default=2)
    parser.add_argument('--latency-samples', type=int, default=0)
    args = parser.parse_args()
    if not 2 <= args.peer_count <= 64:
        parser.error('peer count must be 2–64')
    if not 5 <= args.concurrent_payload_bytes <= 65507:
        parser.error('concurrent payload bytes must be 5–65507')
    if args.concurrent_samples != 0 and not 100 <= args.concurrent_samples <= 10000:
        parser.error('concurrent samples must be zero or 100–10000')
    if args.native_gateway and (not args.mtls or args.guest_ipv6): parser.error('--native-gateway requires --mtls and the IPv4 guest relay')
    if args.mtls and not args.tls: parser.error('--mtls requires --tls')
    if args.latency_samples != 0 and not 100 <= args.latency_samples <= 10000:
        parser.error('latency samples must be zero or 100–10000')
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (args.daemon, args.control_plane, args.cli, args.kernel, args.initrd)}
    if args.native_gateway:
        inputs[str(args.native_gateway)] = hashlib.sha256(args.native_gateway.read_bytes()).hexdigest()
        inputs[str(Path(__file__).resolve())] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
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
    udp_path = 'udp6' if args.guest_ipv6 else 'udp'
    udp_protocol = 'hv2-udp6/1' if args.guest_ipv6 else 'hv2-udp/1'
    peer_family = socket.AF_INET6 if args.local_ipv6 else socket.AF_INET
    peer_host = '::1' if args.local_ipv6 else '127.0.0.1'
    def tunnel(id, credential=token, protocol=None):
        protocol = udp_protocol if protocol is None else protocol
        sock = socket.create_connection(('127.0.0.1', api_port), timeout=5)
        if args.tls: sock = context.wrap_socket(sock, server_hostname='localhost')
        sock.sendall((f'GET /sandboxes/{id}/ports/5353/{udp_path} HTTP/1.1\r\nHost: localhost\r\n'
            f'Connection: Upgrade\r\nUpgrade: {protocol}\r\nX-Api-Key: {credential}\r\n\r\n').encode())
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
    native_process = None
    native_tcp = None
    native_peers = []
    native_address = None
    native_allocation = None
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
        node_tls_arguments = []
        control_mtls_arguments = []
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
            if args.mtls:
                for name, usage in [('hv2-node', 'serverAuth,clientAuth'), ('control', 'clientAuth')]:
                    subprocess.run(['openssl', 'req', '-new', '-newkey', 'ed25519', '-nodes',
                        '-keyout', str(root / (name + '.key')), '-out', str(root / (name + '.csr')),
                        '-subj', '/CN=' + name, '-addext', 'subjectAltName=DNS:' + name,
                        '-addext', 'basicConstraints=critical,CA:FALSE', '-addext', 'extendedKeyUsage=' + usage],
                        capture_output=True, check=True, timeout=20)
                    subprocess.run(['openssl', 'x509', '-req', '-in', str(root / (name + '.csr')),
                        '-CA', str(root / 'ca.pem'), '-CAkey', str(root / 'ca.key'), '-CAcreateserial',
                        '-copy_extensions', 'copy', '-days', '1', '-out', str(root / (name + '.pem'))],
                        capture_output=True, check=True, timeout=20)
                node_tls_arguments = ['--mtls-ca', str(root / 'ca.pem'), '--mtls-cert', str(root / 'hv2-node.pem'), '--mtls-key', str(root / 'hv2-node.key')]
                control_mtls_arguments = ['--mtls-ca', str(root / 'ca.pem'), '--mtls-cert', str(root / 'control.pem'), '--mtls-key', str(root / 'control.key')]


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
                control = subprocess.Popen([str(args.control_plane), '--store', store_url, '--namespace', namespace, '--port', str(api_port), '--proxy-port', str(port())] + tls_arguments + control_mtls_arguments, stdout=log, stderr=log, env=dict(os.environ, HV2_API_KEY=token, HV2_CLUSTER_TOKEN=cluster_token))
                services.append(control)
                process = subprocess.Popen([str(args.daemon), '--port', str(node_port), '--proxy-port', str(node_proxy),
                    '--cluster-store', store_url, '--cluster-namespace', namespace, '--node-id', 'owned-udp-node',
                    '--advertise-api', f'{"https" if args.mtls else "http"}://127.0.0.1:{node_port}', '--advertise-proxy', f'127.0.0.1:{node_proxy}',
                    '--snapshot-store', str(root / 'snapshots')] + node_tls_arguments, stdout=log, stderr=log,
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
                if args.mtls:
                    try:
                        with socket.create_connection(('127.0.0.1', node_port), timeout=5) as plain:
                            with context.wrap_socket(plain, server_hostname='hv2-node') as anonymous:
                                anonymous.sendall(b'GET /sandboxes HTTP/1.1\r\nHost: hv2-node\r\n\r\n')
                                if anonymous.recv(1): raise ValueError('node accepted a TLS peer without a client certificate')
                    except (ssl.SSLError, ConnectionResetError, BrokenPipeError): pass
                    checks.append('Node mTLS refuses a peer without a client certificate')
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
                    {'cmd': '/bin/hm-udp-echo' + (' --ipv6' if args.guest_ipv6 else '') + ' >/tmp/udp-echo.log 2>&1 &', 'timeout_secs': 5})
                if result['exit_code'] != 0: raise ValueError('echo service launch failed')
                if args.probe_ipv6:
                    probe = api('POST', f'/sandboxes/{sandbox}/exec',
                        {'cmd': '/bin/hm-udp-echo --ipv6-probe', 'timeout_secs': 10})
                    if probe['exit_code'] != 0 or probe.get('timed_out'):
                        raise ValueError('owned guest IPv6 probe failed: ' + probe.get('stderr', ''))
                    details = json.loads(probe['stdout'])
                    if not details.get('ipv6_loopback') or not details.get('exact_datagrams') or details.get('payload_sizes') != [0, 4, 65507]:
                        raise ValueError('guest IPv6 probe did not verify exact bounded datagrams')
                    (args.output / 'ipv6-probe.json').write_text(json.dumps(probe, indent=2) + '\n')
                    checks.append('Guest IPv6 loopback supports exact empty binary and maximum framed payload sizes')
                time.sleep(.1)
                refused, status = tunnel(sandbox, 'wrong-owned-token')
                refused.close()
                if status != 401: raise ValueError('wrong cluster credential accepted')
                checks.append('Wrong control-plane API key refused')
                if args.guest_ipv6:
                    refused, status = tunnel(sandbox, protocol='hv2-udp/1')
                    refused.close()
                    if status != 400: raise ValueError('IPv6 route accepted IPv4 upgrade protocol')
                    checks.append('IPv6 route refuses IPv4 upgrade protocol')
                    node_context = None
                    if args.mtls:
                        node_context = ssl.create_default_context(cafile=root / 'ca.pem')
                        node_context.load_cert_chain(root / 'control.pem', root / 'control.key')
                    for node_credential in ['wrong-owned-cluster-token', None]:
                        with socket.create_connection(('127.0.0.1', node_port), timeout=5) as plain:
                            node_stream = node_context.wrap_socket(plain, server_hostname='hv2-node') if args.mtls else plain
                            with node_stream:
                                credential_header = '' if node_credential is None else f'X-Hv2-Cluster-Token: {node_credential}\r\n'
                                node_stream.sendall((f'GET /sandboxes/{sandbox}/ports/5353/udp6 HTTP/1.1\r\nHost: hv2-node\r\n'
                                    f'Connection: Upgrade\r\nUpgrade: hv2-udp6/1\r\n{credential_header}\r\n').encode())
                                headers = bytearray()
                                while not headers.endswith(b'\r\n\r\n'):
                                    byte = node_stream.recv(1)
                                    if not byte or len(headers) > 8192: raise ValueError('invalid node authentication response')
                                    headers.extend(byte)
                                if int(headers.split(b' ', 2)[1]) != 401: raise ValueError('IPv6 node route accepted missing/wrong cluster credentials')
                    checks.append('IPv6 node route refuses missing and wrong cluster credentials with a valid mTLS client')
                stream, status = tunnel(sandbox)
                streams.append(stream)
                if status != 101: raise ValueError(f'guest UDP setup returned {status}')
                for payload in (b'', b'\x00\xff\r\n', bytes(n % 251 for n in range(65507))): roundtrip(stream, payload)
                checks.append(f'Empty binary and maximum-size IPv{6 if args.guest_ipv6 else 4} datagrams verified through real KVM')
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
                    'udp', sandbox, '--port', '5353', '--max-peers', str(args.peer_count)] + (['--guest-ipv6'] if args.guest_ipv6 else []) + (['--listen', '[::1]:0'] if args.local_ipv6 else []), stdout=subprocess.PIPE, stderr=log,
                    env=dict(os.environ, HV2_API_KEY=token))
                if not select.select([cli_process.stdout], [], [], 5)[0]: raise TimeoutError('CLI readiness')
                announced = json.loads(cli_process.stdout.readline())
                host, local_port = announced['listen'].rsplit(':', 1)
                host = host.strip('[]')
                peers = [socket.socket(peer_family, socket.SOCK_DGRAM) for _ in range(args.peer_count)]
                try:
                    for peer in peers:
                        peer.bind((peer_host, 0)); peer.settimeout(5)
                    for size in (0, 4, 65507):
                        messages = [bytes([index + 1]) * size for index in range(args.peer_count)]
                        for peer, payload in zip(peers, messages):
                            peer.sendto(payload, (host, int(local_port)))
                            if peer.recvfrom(65508)[0] != payload: raise ValueError('real CLI peer payload differs')
                    if args.latency_samples:
                        def cli_transfer(payload):
                            peers[0].sendto(payload, (host, int(local_port)))
                            if peers[0].recvfrom(65508)[0] != payload: raise ValueError('timed CLI bytes differ')
                        measure('cli_loopback_control_api', cli_transfer)
                    if args.concurrent_samples:
                        barrier = threading.Barrier(args.peer_count, timeout=10)
                        def timed_peer(index):
                            tail = bytes(n % 251 for n in range(args.concurrent_payload_bytes - 5))
                            samples = []
                            for sequence in range(args.concurrent_samples + 10):
                                if sequence == 10:
                                    barrier.wait()
                                    measured_started = time.monotonic()
                                payload = bytes([index]) + sequence.to_bytes(4, 'big') + tail
                                started = time.perf_counter_ns()
                                peers[index].sendto(payload, (host, int(local_port)))
                                if peers[index].recvfrom(65508)[0] != payload: raise ValueError('concurrent peer payload mismatch')
                                elapsed = (time.perf_counter_ns() - started) / 1e6
                                if sequence >= 10: samples.append(elapsed)
                            measured_elapsed = time.monotonic() - measured_started
                            ordered = sorted(samples)
                            return {'measured_wall_seconds': measured_elapsed, 'peer': index, 'payload_bytes': args.concurrent_payload_bytes, 'samples_ms': samples,
                                'p50_ms': statistics.median(samples), 'p95_ms': ordered[math.ceil(len(samples)*.95)-1],
                                'p99_ms': ordered[math.ceil(len(samples)*.99)-1]}
                        started = time.monotonic()
                        with ThreadPoolExecutor(max_workers=args.peer_count) as executor:
                            results = list(executor.map(timed_peer, range(args.peer_count)))
                        elapsed = time.monotonic() - started
                        measurements.append({'path': 'cli_two_concurrent_peers' if args.peer_count == 2 else 'cli_concurrent_peers', 'concurrency': args.peer_count,
                            'samples_per_peer': args.concurrent_samples, 'warmups_per_peer': 10, 'peers': results,
                            'measured_roundtrips_per_second': args.peer_count * args.concurrent_samples / max(result['measured_wall_seconds'] for result in results),
                            'wall_seconds_including_warmups': elapsed,
                            'verified_roundtrips_per_second_including_warmups': args.peer_count * (args.concurrent_samples + 10) / elapsed})
                        checks.append('Concurrent CLI peers return exact tagged sequence payloads')
                    if args.idle_check:
                        time.sleep(33)
                        payload = b'after-idle\x00\xff'
                        peers[0].sendto(payload, (host, int(local_port)))
                        if peers[0].recvfrom(65508)[0] != payload: raise ValueError('existing peer failed after idle')
                        with socket.socket(peer_family, socket.SOCK_DGRAM) as new_peer:
                            new_peer.bind((peer_host, 0)); new_peer.settimeout(5)
                            new_peer.sendto(payload, (host, int(local_port)))
                            if new_peer.recvfrom(65508)[0] != payload: raise ValueError('idle peer slot was not released')
                        checks.append('Idle CLI sessions release peer slots and accept existing/new peers')


                finally:
                    for peer in peers: peer.close()
                cli_process.send_signal(signal.SIGINT)
                if cli_process.wait(timeout=5) != 0: raise ValueError('CLI interrupt failed')
                checks.append(f'{args.peer_count} CLI peers preserve empty binary and maximum-size datagrams through real control plane Redis node and KVM')

                if args.native_gateway:
                    result = api('POST', f'/sandboxes/{sandbox}/exec',
                        {'cmd': '/bin/tcp-fixture >/tmp/native-tcp.log 2>&1 & /bin/hm-udp-echo --port 18082 >/tmp/native-udp.log 2>&1 &', 'timeout_secs': 5})
                    if result['exit_code'] != 0: raise ValueError('native guest fixture launch failed')
                    prefix = 'hv2:' + namespace
                    def redis_command(*values):
                        return subprocess.run(['redis-cli','--raw','-h','127.0.0.1','-p',str(redis_port),*map(str,values)], capture_output=True, text=True, check=True, timeout=5).stdout.strip()
                    public_port = port()
                    native_address = (peer_host, public_port)
                    native_allocation = {'sandbox_id':sandbox,'machine_port':18082,'public_port':public_port,'owner_id':'owned-native-principal','protocol':'both'}
                    def reserve_native():
                        encoded = json.dumps(native_allocation, separators=(',',':'))
                        redis_command('EVAL', "redis.call('HSET',KEYS[1],ARGV[1],ARGV[3]);redis.call('HSET',KEYS[2],ARGV[2],ARGV[3]);return 1", 2,
                            prefix + ':public-ports', prefix + ':ports:' + sandbox, public_port, 18082, encoded)
                    def native_connect():
                        deadline = time.monotonic() + 8
                        while True:
                            if native_process.poll() is not None: raise ValueError('native gateway exited')
                            try:
                                client = socket.create_connection(native_address, timeout=.5); client.settimeout(5); streams.append(client); return client
                            except OSError:
                                if time.monotonic() > deadline: raise TimeoutError('native gateway listener readiness')
                                time.sleep(.02)
                    def native_tcp_echo(client, payload):
                        # Guest streaming fixture echoes before half-close. Own
                        # the writer thread so backpressure cannot deadlock reads.
                        errors = []
                        def send():
                            try: client.sendall(payload)
                            except OSError: errors.append('native TCP send failed')
                        sender = threading.Thread(target=send); sender.start()
                        try:
                            if exact(client, len(payload)) != payload: raise ValueError('native guest TCP payload differs')
                        finally: sender.join(8)
                        if sender.is_alive() or errors: raise ValueError('native TCP writer failed')
                    def native_udp_echo(peer, payload):
                        deadline = time.monotonic() + 8
                        while True:
                            peer.sendto(payload, native_address)
                            try:
                                received, source = peer.recvfrom(65508)
                                if received != payload or (source[0], source[1]) != native_address: raise ValueError('native guest UDP payload/source differs')
                                return
                            except socket.timeout:
                                if time.monotonic() > deadline: raise TimeoutError('native guest UDP readiness/recovery')
                    def native_absent():
                        deadline = time.monotonic() + 5
                        while True:
                            try: probe = socket.create_connection(native_address, timeout=.2); probe.close()
                            except OSError: return
                            if time.monotonic() > deadline: raise TimeoutError('native listener remained bound')
                            time.sleep(.02)
                    reserve_native()
                    native_process = subprocess.Popen([str(args.native_gateway),'--bind-ip',peer_host,'--namespace',namespace,
                        '--mtls-ca',str(root/'ca.pem'),'--mtls-cert',str(root/'control.pem'),'--mtls-key',str(root/'control.key'),
                        '--max-ports','8','--max-sessions',str(args.peer_count+2),'--udp-peers',str(args.peer_count),'--poll-ms','20'],
                        env=dict(os.environ,HV2_STORE_URL=store_url,HV2_CLUSTER_TOKEN=cluster_token),stdout=log,stderr=log)
                    services.append(native_process)
                    native_tcp = native_connect(); native_tcp_echo(native_tcp, bytes(range(256))*4096)
                    for index in range(args.peer_count):
                        peer = socket.socket(peer_family,socket.SOCK_DGRAM); peer.bind((peer_host,0)); peer.settimeout(.5); native_peers.append(peer)
                        for payload in [b'',bytes([index+1,0,255,13]),bytes([index+1])*65507]: native_udp_echo(peer,payload)
                    checks.append(f'Native gateway real KVM TCP preserves 1 MiB and {args.peer_count} UDP peers preserve empty binary and maximum datagrams on one public port')
                    native_allocation['protocol'] = 'udp'; reserve_native(); closed(native_tcp); native_absent()
                    native_udp_echo(native_peers[0],b'native-protocol-update')
                    native_allocation['protocol'] = 'both'; reserve_native(); native_tcp = native_connect(); native_tcp_echo(native_tcp,b'native-both-restored')
                    checks.append('Native real guest protocol updates retain public port and close old TCP')

                stream, status = tunnel(sandbox)
                streams.append(stream)
                if status != 101: raise ValueError('pre-pause session setup failed')
                roundtrip(stream, b'live-before-pause')
                api('POST', f'/sandboxes/{sandbox}/pause', {})
                closed(stream)
                checks.append('Pause closes existing UDP session')
                if args.native_gateway:
                    closed(native_tcp); native_absent()
                    if json.loads(redis_command('HGET', prefix+':public-ports', public_port)) != native_allocation: raise ValueError('pause changed native reservation')
                    checks.append('Real guest pause closes native sessions and preserves exact public reservation')
                api('POST', f'/sandboxes/{sandbox}/resume', {})
                resumed, status = tunnel(sandbox)
                streams.append(resumed)
                if status != 101: raise ValueError('resumed UDP setup failed')
                roundtrip(resumed, b'after-resume\x00\xff')
                checks.append('Resumed guest accepts a new UDP session')
                if args.native_gateway:
                    native_tcp = native_connect(); native_tcp_echo(native_tcp,b'native-after-resume')
                    native_udp_echo(native_peers[0],b'native-after-resume\x00\xff')
                    checks.append('Real guest resume restores native TCP and UDP on same reserved port')
                api('DELETE', f'/sandboxes/{sandbox}')
                sandbox = None
                closed(resumed)
                if api('GET', '/sandboxes') != []: raise ValueError('guest inventory not empty')
                checks.append('Delete closes UDP session and clears guest inventory')
                if args.native_gateway:
                    closed(native_tcp); native_absent()
                    deadline = time.monotonic()+5
                    while redis_command('HLEN',prefix+':public-ports') != '0':
                        if time.monotonic()>deadline: raise TimeoutError('real guest deletion left native reservation')
                        time.sleep(.02)
                    native_process.send_signal(signal.SIGTERM)
                    if native_process.wait(timeout=8) != 0: raise ValueError('native gateway graceful shutdown failed')
                    checks.append('Real guest deletion removes native allocation and closes native sessions; gateway stops gracefully')
            finally:
                for peer in native_peers: peer.close()
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
        'native_gateway_reaped': native_process is None or native_process.poll() is not None,
        'native_gateway_real_kvm': bool(args.native_gateway),
        'guest_udp_ipv6': args.guest_ipv6, 'local_udp_ipv6': args.local_ipv6,
        'control_api_tls': args.tls, 'node_mtls': args.mtls, 'latency_measurements': measurements,
        'scope': 'Owned shipped CLI/control-plane/Redis/daemon/KVM UDP; optional verified API TLS/node mTLS; Redis local plaintext.'}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
