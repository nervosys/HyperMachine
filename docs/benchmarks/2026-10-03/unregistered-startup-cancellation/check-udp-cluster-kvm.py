#!/usr/bin/env python3
"""Verify framed UDP through an owned shipped CLI/control-plane/Redis/KVM stack."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import http.client
import json
import math
import statistics
import os
import platform
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
    parser.add_argument('--owner-context', action='store_true')
    parser.add_argument('--owner-port-api', action='store_true')
    parser.add_argument('--owner-port-cli', action='store_true')
    parser.add_argument('--owner-adoption', action='store_true')
    parser.add_argument('--resume-publication-fault', action='store_true')
    parser.add_argument('--recovery-cli', action='store_true')
    parser.add_argument('--automatic-registration-recovery', action='store_true')
    parser.add_argument('--named-registration-recovery', action='store_true')
    parser.add_argument('--registration-recovery-timeout', action='store_true')
    parser.add_argument('--creation-publication-fault', action='store_true')
    parser.add_argument('--pending-discovery', action='store_true')
    parser.add_argument('--publisher-race', action='store_true')
    parser.add_argument('--resume-publication-fault-command', choices=('SET','XADD'), default='SET')
    parser.add_argument('--in-memory-pauses', action='store_true', help='Omit shared snapshot store to exercise local paused-state lifecycle')
    parser.add_argument('--owner-adoption-cross-node', action='store_true')
    parser.add_argument('--native-port-migration', action='store_true')
    parser.add_argument('--native-capacity', action='store_true')
    parser.add_argument('--native-tcp-capacity', action='store_true')
    parser.add_argument('--native-mixed-capacity', action='store_true')
    parser.add_argument('--native-comparison-samples', type=int, default=0)
    parser.add_argument('--native-resource-samples', action='store_true')
    parser.add_argument('--host-cpus', help='Comma-separated allowed Linux CPU IDs; inherited by owned children, not exclusive CPU reservation')
    parser.add_argument('--native-comparison-bytes', type=int, default=4096)
    parser.add_argument('--host-build-profile', choices=('development', 'release'), default='development',
        help='Operator-declared host build provenance; not inferred from executable bytes')
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
    if args.native_mixed_capacity and not args.native_tcp_capacity: parser.error('--native-mixed-capacity requires --native-tcp-capacity')
    if args.native_tcp_capacity and not args.native_capacity: parser.error('--native-tcp-capacity requires --native-capacity')
    if args.native_capacity and not args.owner_port_cli: parser.error('--native-capacity requires --owner-port-cli')
    if args.owner_port_cli and not args.owner_port_api: parser.error('--owner-port-cli requires --owner-port-api')
    if args.native_resource_samples and not args.native_comparison_samples: parser.error('--native-resource-samples requires --native-comparison-samples')
    if args.native_port_migration and not args.owner_adoption_cross_node: parser.error('--native-port-migration requires --owner-adoption-cross-node')
    if args.owner_adoption_cross_node and not args.owner_adoption: parser.error('--owner-adoption-cross-node requires --owner-adoption')
    if args.owner_adoption and not args.owner_port_cli: parser.error('--owner-adoption requires --owner-port-cli')
    if args.owner_port_api and not args.owner_context: parser.error('--owner-port-api requires --owner-context')
    if args.owner_context and (not args.native_gateway or not args.mtls): parser.error('--owner-context requires --native-gateway and --mtls')
    if args.mtls and not args.tls: parser.error('--mtls requires --tls')
    if args.latency_samples != 0 and not 100 <= args.latency_samples <= 10000:
        parser.error('latency samples must be zero or 100–10000')
    if args.native_comparison_samples and (not args.native_gateway or not 100 <= args.native_comparison_samples <= 10000):
        parser.error('native comparison requires --native-gateway and 100–10000 samples')
    if not 6 <= args.native_comparison_bytes <= 65507:
        parser.error('native comparison bytes must be 6–65507')
    requested_cpus = None
    if args.host_cpus is not None:
        try:
            parts = args.host_cpus.split(',')
            if any(not part.isascii() or not part.isdecimal() for part in parts):
                raise ValueError('CPU IDs must be comma-separated nonnegative integers')
            requested_cpus = {int(part) for part in parts}
            if len(requested_cpus) != len(parts):
                raise ValueError('duplicate CPU IDs')
            if not requested_cpus <= os.sched_getaffinity(0):
                raise ValueError('requested CPU is outside the current allowed affinity')
            os.sched_setaffinity(0, requested_cpus)
            if os.sched_getaffinity(0) != requested_cpus:
                raise ValueError('applied affinity differs from requested CPUs')
        except (ValueError, OSError, AttributeError) as error:
            parser.error(str(error))
    if args.in_memory_pauses and args.owner_adoption:
        parser.error('--in-memory-pauses cannot use shared-store adoption/migration checks')
    if args.resume_publication_fault and not args.owner_context: parser.error('--resume-publication-fault requires --owner-context')
    if args.recovery_cli and not (args.resume_publication_fault and args.owner_port_api): parser.error('--recovery-cli requires resume fault and owner-port API fixtures')
    if args.named_registration_recovery and not args.automatic_registration_recovery: parser.error('--named-registration-recovery requires automatic recovery')
    if args.registration_recovery_timeout and not args.automatic_registration_recovery: parser.error('--registration-recovery-timeout requires automatic recovery')
    if args.automatic_registration_recovery and not (args.creation_publication_fault and args.pending_discovery): parser.error('--automatic-registration-recovery requires initial fault and pending discovery')
    if args.creation_publication_fault and not args.recovery_cli: parser.error('--creation-publication-fault requires --recovery-cli')
    if args.pending_discovery and not args.creation_publication_fault: parser.error('--pending-discovery requires initial creation fault fixture')
    if args.publisher_race and not args.creation_publication_fault: parser.error('--publisher-race requires creation fault fixture')
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (args.daemon, args.control_plane, args.cli, args.kernel, args.initrd)}
    if args.native_gateway:
        inputs[str(args.native_gateway)] = hashlib.sha256(args.native_gateway.read_bytes()).hexdigest()
        inputs[str(Path(__file__).resolve())] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    def port():
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            return sock.getsockname()[1]
    managed_public_port = port() if args.owner_port_api else None
    api_port = port()
    node_port = port()
    node_proxy = port()
    redis_port = port()
    if args.native_capacity:
        family=socket.AF_INET6 if args.local_ipv6 else socket.AF_INET
        host='::1' if args.local_ipv6 else '127.0.0.1'
        for attempt in range(100):
            first=48000+secrets.randbelow(14000)
            if any(first<=value<=first+16 for value in [api_port,node_port,node_proxy,redis_port]): continue
            held=[]
            try:
                for value in range(first,first+17):
                    for kind in [socket.SOCK_STREAM,socket.SOCK_DGRAM]:
                        probe=socket.socket(family,kind);held.append(probe);probe.bind((host,value))
                managed_public_port=first
                break
            except OSError: pass
            finally:
                for probe in held: probe.close()
        else: raise ValueError('no owned seventeen-port window available')
    store_url = f'redis://127.0.0.1:{redis_port}'
    namespace = secrets.token_hex(8)
    prefix = 'hv2:' + namespace
    def redis_command(*values):
        return subprocess.run(['redis-cli','--raw','-h','127.0.0.1','-p',str(redis_port),*map(str,values)], capture_output=True, text=True, check=True, timeout=5).stdout.strip()
    def verify_owner(id, phase):
        record = json.loads(redis_command('GET', prefix + ':sandbox:' + id))
        if record.get('owner_id') != 'owned-principal-a': raise ValueError('trusted creator ownership differs')
        owner_checks.append({'phase': phase, 'owner_id': record['owner_id']})

    cluster_token = secrets.token_hex(32)
    token = secrets.token_hex(32)
    fixture_credentials = [token, cluster_token]
    base = f'{"https" if args.tls else "http"}://localhost:{api_port}'
    context = None
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def api(method, path, body=None, extra_headers=None):
        request = urllib.request.Request(base + path, method=method,
            headers={'X-Api-Key': token, 'Content-Type': 'application/json', **(extra_headers or {})},
            data=None if body is None else json.dumps(body).encode())
        with opener.open(request, timeout=30) as response:
            data = response.read()
            return json.loads(data) if data else None
    def status_api(method, path, credential, body=None, extra_headers=None):
        request = urllib.request.Request(base+path, method=method,
            headers={'X-Api-Key':credential,'Content-Type':'application/json',**(extra_headers or {})},
            data=None if body is None else json.dumps(body).encode())
        try:
            with opener.open(request,timeout=15) as response: return response.status,response.read()
        except urllib.error.HTTPError as error:
            return error.code,error.read()
    udp_path = 'udp6' if args.guest_ipv6 else 'udp'
    udp_protocol = 'hv2-udp6/1' if args.guest_ipv6 else 'hv2-udp/1'
    peer_family = socket.AF_INET6 if args.local_ipv6 else socket.AF_INET
    peer_host = '::1' if args.local_ipv6 else '127.0.0.1'
    def tunnel(id, credential=None, protocol=None):
        credential = token if credential is None else credential
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
    comparison_peers = []
    native_comparison = []
    guest_resources = None
    creation_owner = None
    owned_children = []
    owner_checks = []
    capacity_peers = []
    capacity_tcp = []
    capacity_rows = []
    capacity_snapshot = None
    capacity_report = None
    secondary_process = None
    migration_report = None
    port_migration_report = None
    native_resources = []
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
                owner_arguments = []
                control_api_key = token
                if args.owner_context:
                    policy_path = root / 'owner-policies.json'
                    owner_policies = [{'sha256':hashlib.sha256(token.encode()).hexdigest(),
                        'expires_at':int(time.time())+600,'scopes':['admin'],'principal_id':'owned-principal-a'}]
                    if args.owner_port_api:
                        other_key, observer_key, inventory_key, unassigned_key = [secrets.token_hex(32) for _ in range(4)]
                        sandboxes_key=secrets.token_hex(32)
                        fixture_credentials.extend([other_key,observer_key,inventory_key,unassigned_key,sandboxes_key])
                        owner_policies.append({'sha256':hashlib.sha256(sandboxes_key.encode()).hexdigest(),'expires_at':int(time.time())+600,'scopes':['sandboxes'],'role':'operator','principal_id':'owned-principal-a'})
                        for key,principal,role,scopes in [(other_key,'owned-principal-b','operator',['admin']),
                            (observer_key,'owned-principal-a','observer',['admin']),
                            (inventory_key,'owned-principal-a','operator',['inventory']),
                            (unassigned_key,None,'operator',['admin'])]:
                            owner_policies.append({'sha256':hashlib.sha256(key.encode()).hexdigest(),'expires_at':int(time.time())+600,
                                'scopes':scopes,'role':role,'principal_id':principal})
                        owner_arguments = ['--native-port-range', f'{managed_public_port}-{managed_public_port+(16 if args.native_capacity else 0)}']
                    policy_path.write_text(json.dumps(owner_policies))
                    policy_path.chmod(0o600)
                    owner_arguments += ['--api-keys-file', str(policy_path)]
                    control_api_key = secrets.token_hex(32)
                    fixture_credentials.append(control_api_key)
                control = subprocess.Popen([str(args.control_plane), '--store', store_url, '--namespace', namespace, '--port', str(api_port), '--proxy-port', str(port())] + tls_arguments + control_mtls_arguments + owner_arguments, stdout=log, stderr=log, env=dict(os.environ, HV2_API_KEY=control_api_key, HV2_CLUSTER_TOKEN=cluster_token))
                services.append(control)
                process = subprocess.Popen([str(args.daemon), '--port', str(node_port), '--proxy-port', str(node_proxy),
                    '--cluster-store', store_url, '--cluster-namespace', namespace, '--node-id', 'owned-udp-node',
                    '--advertise-api', f'{"https" if args.mtls else "http"}://127.0.0.1:{node_port}', '--advertise-proxy', f'127.0.0.1:{node_proxy}',
                    ] + (['--registration-reconcile-interval','1'] if args.automatic_registration_recovery else []) + ([] if args.in_memory_pauses else ['--snapshot-store', str(root / 'snapshots')]) + node_tls_arguments, stdout=log, stderr=log,
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
                    # Shared template publication can precede the node API listener.
                    listener_deadline=time.monotonic()+5
                    while True:
                        try:
                            with socket.create_connection(('127.0.0.1',node_port),timeout=.2):break
                        except ConnectionRefusedError:
                            if process.poll() is not None:raise ValueError('daemon exited before node API readiness')
                            if time.monotonic()>listener_deadline:raise TimeoutError('node API listener readiness')
                            time.sleep(.02)
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
                if args.owner_context:
                    node_context = ssl.create_default_context(cafile=str(root/'ca.pem'))
                    node_context.load_cert_chain(root/'control.pem', root/'control.key')
                    for credential in (None, 'wrong-cluster-token'):
                        body = json.dumps({'templateID':'base'}).encode()
                        extra = '' if credential is None else f'X-Hv2-Cluster-Token: {credential}\r\n'
                        with socket.create_connection(('127.0.0.1',node_port),timeout=5) as raw:
                            with node_context.wrap_socket(raw,server_hostname='hv2-node') as client:
                                client.sendall((f'POST /sandboxes HTTP/1.1\r\nHost: hv2-node\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nX-Hv2-Sandbox-Owner: spoofed-principal\r\n{extra}\r\n').encode()+body)
                                headers = bytearray()
                                while not headers.endswith(b'\r\n\r\n'):
                                    byte = client.recv(1)
                                    if not byte or len(headers)>8192: raise ValueError('invalid node creator refusal response')
                                    headers.extend(byte)
                                if int(headers.split(b' ',2)[1]) != 401: raise ValueError('node accepted unauthenticated creator context')
                    checks.append('Valid mTLS node peers cannot create with forged owner context and missing or wrong cluster credentials')
                if args.creation_publication_fault:
                    def creation_node(method, path, payload=None, credential=cluster_token):
                        connection=http.client.HTTPSConnection('127.0.0.1',node_port,context=node_context,timeout=30)
                        connection.connect=lambda:setattr(connection,'sock',node_context.wrap_socket(socket.create_connection(('127.0.0.1',node_port),timeout=30),server_hostname='hv2-node'))
                        try:
                            connection.request(method,path,body=json.dumps(payload or {}).encode(),headers={'Content-Type':'application/json',
                                'X-Hv2-Cluster-Token':credential,'X-Hv2-Sandbox-Owner':'owned-principal-a'})
                            response=connection.getresponse();body=response.read();return response.status,body
                        finally:connection.close()
                    def discover_cli(key, node='owned-udp-node', after=None):
                        command=[str(args.cli),'sandbox','vm','--endpoint',base,'--api-ca-cert',str(root/'ca.pem'),
                            'pending-registrations','--node-id',node]
                        if after is not None:command+=['--after',after]
                        result=subprocess.run(command,env=dict(os.environ,HV2_API_KEY=key),capture_output=True,timeout=30)
                        if any(secret.encode() in result.stdout+result.stderr for secret in (token,cluster_token,control_api_key,other_key,observer_key,inventory_key,sandboxes_key)):raise ValueError('pending discovery leaked API credentials')
                        return result
                    def require_pending(expected, kind='unnamed'):
                        result=discover_cli(token)
                        if result.returncode!=0:raise ValueError('pending discovery CLI failed')
                        page=json.loads(result.stdout)
                        if set(page)!=set(('registrations','nextCursor')) or page['nextCursor'] is not None:raise ValueError('unexpected single-guest discovery page')
                        rows=page['registrations']
                        if any(set(row)!=set(('sandboxID','kind')) or row['kind']!=kind for row in rows):raise ValueError('pending page includes capability fields or wrong kind')
                        if {row['sandboxID'] for row in rows}!=expected:raise ValueError('pending discovery guest IDs differ')
                    if args.publisher_race:
                        baseline={row['sandboxID'] for row in json.loads(creation_node('GET','/sandboxes')[1])}
                        redis_command('CLIENT','PAUSE',1000,'WRITE')
                        try:
                            with ThreadPoolExecutor(max_workers=2) as executor:
                                publishing=executor.submit(creation_node,'POST','/sandboxes',{'templateID':'base','timeout':300})
                                deadline=time.monotonic()+.8
                                while True:
                                    ids={row['sandboxID'] for row in json.loads(creation_node('GET','/sandboxes')[1])}-baseline
                                    if ids:break
                                    if publishing.done() or time.monotonic()>deadline:raise ValueError('publication hold did not expose live pending guest')
                                    time.sleep(.005)
                                if len(ids)!=1 or publishing.done():raise ValueError('initial publisher was not observed active')
                                held=ids.pop()
                                reconciling=executor.submit(creation_node,'POST',f'/sandboxes/{held}/registration/reconcile')
                                time.sleep(.05)
                                if publishing.done() or reconciling.done():raise ValueError('controlled requests did not overlap while publication held')
                                redis_command('CLIENT','UNPAUSE')
                                status,body=publishing.result(timeout=30)
                                if status not in (200,201) or json.loads(body)['sandboxID']!=held:raise ValueError('original held publication did not complete')
                                status,body=reconciling.result(timeout=30)
                                if status!=409 or b'no uncertain local registration' not in body:raise ValueError('serialized reconciliation did not observe completed publication')
                        finally:redis_command('CLIENT','UNPAUSE')
                        require_pending(set())
                        api('DELETE',f'/sandboxes/{held}')
                        checks.append('Controlled owned Redis write hold exposes publishing KVM guest; concurrent reconciliation waits while original publisher owns transition lock, then completed publication clears marker and reconciliation returns 409')
                    before={row['sandboxID'] for row in json.loads(creation_node('GET','/sandboxes')[1])}
                    redis_command('ACL','SETUSER','default','-'+args.resume_publication_fault_command.lower())
                    try:
                        initial_body={'templateID':'base','timeout':300}
                        initial_name='owned-recovery-'+secrets.token_hex(8)
                        if args.named_registration_recovery:
                            initial_body['metadata']={'hm.name':initial_name}
                            creation_status=status_api('POST','/sandboxes',token,initial_body)[0]
                        else:creation_status=creation_node('POST','/sandboxes',initial_body)[0]
                        if creation_status!=503:raise ValueError('failed initial registration did not return uncertainty')
                        after={row['sandboxID'] for row in json.loads(creation_node('GET','/sandboxes')[1])}
                        preserved=after-before
                        if len(preserved)!=1:raise ValueError('initial uncertainty did not preserve exactly one local guest')
                        preserved=preserved.pop()
                        stored=redis_command('GET',prefix+':sandbox:'+preserved)
                        if args.resume_publication_fault_command=='SET' and stored:raise ValueError('refused initial SET created shared record')
                        if args.resume_publication_fault_command=='XADD' and (not stored or json.loads(stored).get('paused')):raise ValueError('initial event failure lost committed running record')
                        if creation_node('POST',f'/sandboxes/{preserved}/pause')[0]!=409:raise ValueError('uncertain initial guest allowed pause')
                        if args.named_registration_recovery:
                            original_reservation=json.loads(redis_command('HGET',prefix+':name-reservations',initial_name))
                            expected_binding=preserved if args.resume_publication_fault_command=='XADD' else None
                            if original_reservation['sandbox_id']!=expected_binding:raise ValueError('named failure binding differs from commit stage')
                            if status_api('POST','/sandboxes',token,initial_body)[0]!=409:raise ValueError('pending named creation permitted duplicate guest')
                            if {row['sandboxID'] for row in json.loads(creation_node('GET','/sandboxes')[1])}!=after:raise ValueError('named refusal created another local guest')
                            checks.append('Named uncertainty preserves exact operation reservation; duplicate API creation refuses without another KVM guest')
                    finally:
                        if not args.automatic_registration_recovery:redis_command('ACL','SETUSER','default','+'+args.resume_publication_fault_command.lower())
                    if args.pending_discovery:
                        require_pending({preserved},'named' if args.named_registration_recovery else 'unnamed')
                        for key in (observer_key,inventory_key,sandboxes_key):
                            if discover_cli(key).returncode==0:raise ValueError('nonadministrator discovered registrations')
                        if discover_cli(token,'unregistered-node').returncode==0:raise ValueError('discovery selected unknown node')
                        cursor=discover_cli(token,after=preserved)
                        if cursor.returncode!=0 or json.loads(cursor.stdout)['registrations']:raise ValueError('exclusive discovery cursor failed')
                        if creation_node('GET','/registrations/pending',credential='wrong-cluster-token')[0]!=401:raise ValueError('node discovery bypassed cluster authentication')
                    if args.automatic_registration_recovery:
                        time.sleep(1.2)
                        require_pending({preserved},'named' if args.named_registration_recovery else 'unnamed')
                        if args.registration_recovery_timeout:redis_command('CLIENT','PAUSE','8000','WRITE')
                        redis_command('ACL','SETUSER','default','+'+args.resume_publication_fault_command.lower())
                        if args.registration_recovery_timeout:
                            try:
                                deadline=time.monotonic()+7
                                while sum('registration reconciliation remains uncertain' in line and 'cluster store: timed out' in line and preserved in line for line in (args.output/'daemon.log').read_text(errors='replace').splitlines())<2:
                                    if time.monotonic()>deadline:raise TimeoutError('worker did not retry after bounded store timeouts')
                                    time.sleep(.05)
                                require_pending({preserved},'named' if args.named_registration_recovery else 'unnamed')
                                checks.append('Repeated bounded store timeouts retain pending '+('initial' if preserved==preserved else 'resumed')+' KVM guest during owned Redis write stall')
                            finally:redis_command('CLIENT','UNPAUSE')
                        deadline=time.monotonic()+20
                        while json.loads(creation_node('GET','/registrations/pending')[1])['registrations']:
                            if time.monotonic()>deadline:raise TimeoutError('automatic initial registration recovery')
                            time.sleep(.05)
                        checks.append('Automatic worker recovers initial preserved KVM guest after Redis refusal without a reconciliation request')
                    else:
                        recovered=subprocess.run([str(args.cli),'sandbox','vm','--endpoint',base,'--api-ca-cert',str(root/'ca.pem'),
                            'reconcile-registration',preserved,'--node-id','owned-udp-node'],env=dict(os.environ,HV2_API_KEY=token),capture_output=True,timeout=30)
                        if recovered.returncode!=0 or json.loads(recovered.stdout).get('sandboxID')!=preserved:raise ValueError('CLI did not recover initial preserved guest')
                        if any(secret.encode() in recovered.stdout+recovered.stderr for secret in (token,cluster_token,control_api_key)):raise ValueError('initial recovery leaked API credentials')
                    record=json.loads(redis_command('GET',prefix+':sandbox:'+preserved))
                    if record.get('owner_id')!='owned-principal-a' or record.get('paused'):raise ValueError('initial recovered owner record differs')
                    if args.named_registration_recovery:
                        recovered_reservation=json.loads(redis_command('HGET',prefix+':name-reservations',initial_name))
                        if recovered_reservation['token']!=original_reservation['token'] or recovered_reservation['sandbox_id']!=preserved or record['metadata'].get('hm.name')!=initial_name:raise ValueError('automatic named recovery changed operation or binding')
                    if args.pending_discovery:
                        require_pending(set())
                        checks.append('Administrator CLI discovers initial pending KVM guest without shared record; scoped refusals exclusive cursor and post-recovery disappearance verified')
                    result=api('POST',f'/sandboxes/{preserved}/exec',{'cmd':'/bin/busybox printf initial-recovery-ok','timeout_secs':5})
                    if result.get('stdout')!='initial-recovery-ok' or result.get('exit_code')!=0:raise ValueError('initial preserved guest execution differs')
                    api('DELETE',f'/sandboxes/{preserved}')
                    if preserved in {row['sandboxID'] for row in json.loads(creation_node('GET','/sandboxes')[1])}:raise ValueError('initial guest deletion failed')
                    if args.named_registration_recovery:
                        if redis_command('HGET',prefix+':name-reservations',initial_name):raise ValueError('named deletion retained reservation')
                        reused=api('POST','/sandboxes',initial_body)['sandboxID']
                        if reused==preserved or json.loads(redis_command('HGET',prefix+':name-reservations',initial_name))['sandbox_id']!=reused:raise ValueError('deleted name did not bind a fresh guest')
                        api('DELETE',f'/sandboxes/{reused}')
                        if redis_command('HGET',prefix+':name-reservations',initial_name):raise ValueError('reused named guest deletion retained reservation')
                        checks.append('Automatic named recovery retains operation token and guest identity; deletion releases name and fresh API reuse rebinds then cleans up')
                    checks.append('Initial '+('named' if args.named_registration_recovery else 'unnamed')+' Redis refusal preserves one KVM guest; '+('automatic worker' if args.automatic_registration_recovery else 'CLI explicit-node')+' recovery works with missing SET record or committed XADD record, exact execution and deletion')
                creation_body = {'templateID': 'base', 'timeout': 300}
                if args.owner_context:
                    creation_body.update({'owner_id':'spoofed-principal','metadata':{'owner_id':'spoofed-principal'}})
                sandbox = api('POST', '/sandboxes', creation_body,
                    {'x-hv2-sandbox-owner':'spoofed-principal'} if args.owner_context else None)['sandboxID']
                creation_owner = json.loads(redis_command('GET', prefix + ':sandbox:' + sandbox)).get('owner_id')
                if not args.owner_context and creation_owner is not None: raise ValueError('legacy creation gained owner context')
                if args.owner_context:
                    verify_owner(sandbox, 'created-despite-client-forgery')
                if args.owner_port_cli:
                    api('DELETE',f'/sandboxes/{sandbox}'); sandbox=None
                    create_key=control_api_key if args.owner_adoption else token
                    created=subprocess.run([str(args.cli),'sandbox','vm','--endpoint',base,'--api-ca-cert',str(root/'ca.pem'),
                        'create','--template','base','--lifetime','300'],env=dict(os.environ,HV2_API_KEY=create_key),capture_output=True,timeout=30)
                    if created.returncode!=0 or create_key.encode() in created.stdout+created.stderr: raise ValueError('CLI owned VM creation failed or exposed API credential')
                    descriptor=json.loads(created.stdout);sandbox=descriptor['sandboxID']
                    if descriptor.get('envdAccessToken'): fixture_credentials.append(descriptor['envdAccessToken'])
                    if args.owner_adoption:
                        if json.loads(redis_command('GET',prefix+':sandbox:'+sandbox)).get('owner_id') is not None: raise ValueError('CLI legacy VM unexpectedly had an owner')
                        adopted=subprocess.run([str(args.cli),'sandbox','vm','--endpoint',base,'--api-ca-cert',str(root/'ca.pem'),
                            'adopt-owner',sandbox,'--principal-id','owned-principal-a'],env=dict(os.environ,HV2_API_KEY=control_api_key),capture_output=True,timeout=15)
                        if adopted.returncode!=0 or adopted.stdout.strip() or control_api_key.encode() in adopted.stdout+adopted.stderr: raise ValueError('main CLI adoption failed')
                        verify_owner(sandbox,'cli-created-legacy-adopted')
                        checks.append('Shipped CLI V2 legacy creation and administrator adoption establish owner before real native forwarding')
                    else:
                        verify_owner(sandbox,'cli-created')
                        checks.append('Shipped CLI V2 creation records the configured creator before public port management')
                inventory = api('GET', '/sandboxes')
                guest = next(row for row in inventory if row['sandboxID'] == sandbox)
                guest_resources = {key: guest[key] for key in ('cpuCount', 'memoryMB')}
                if any(not isinstance(value, int) or value <= 0 for value in guest_resources.values()):
                    raise ValueError('invalid measured guest resources')
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
                        {'cmd': '/bin/tcp-fixture '+('--capacity ' if args.native_tcp_capacity else '')+'>/tmp/native-tcp.log 2>&1 & /bin/hm-udp-echo --port 18082 >/tmp/native-udp.log 2>&1 &', 'timeout_secs': 5})
                    if result['exit_code'] != 0: raise ValueError('native guest fixture launch failed')
                    public_port = managed_public_port if args.owner_port_api else port()
                    native_address = (peer_host, public_port)
                    native_allocation = {'sandbox_id':sandbox,'machine_port':18082,'public_port':public_port,'owner_id':'owned-principal-a' if args.owner_context else 'owned-native-principal','protocol':'both'}
                    public_api_path = f'/sandboxes/{sandbox}/public-ports'
                    def public_cli(action, credential=None, target=None, protocol=None, expect_status=None, destination=18082):
                        credential = token if credential is None else credential
                        command = [str(args.cli),'sandbox','vm','--endpoint',base,'--api-ca-cert',str(root/'ca.pem'),
                            'public-ports',action,sandbox if target is None else target]
                        if action != 'list': command += ['--port',str(destination)]
                        if action == 'expose' and protocol is not None: command += ['--protocol',protocol]
                        result = subprocess.run(command,env=dict(os.environ,HV2_API_KEY=credential),capture_output=True,timeout=15)
                        if credential.encode() in result.stdout+result.stderr: raise ValueError('public CLI exposed API credential')
                        if expect_status is not None:
                            if result.returncode == 0 or str(expect_status).encode() not in result.stderr:
                                raise ValueError('public CLI did not report expected refusal')
                            return None
                        if result.returncode != 0: raise ValueError('public CLI operation failed')
                        return json.loads(result.stdout) if result.stdout.strip() else None

                    def reserve_native():
                        if args.owner_port_api:
                            if args.owner_port_cli:
                                value=public_cli('expose',protocol=native_allocation['protocol'])
                                status,body=202,json.dumps(value).encode()
                            else:
                                status,body = status_api('PUT',public_api_path+'/18082',token,{'protocol':native_allocation['protocol']})
                            expected = {'machinePort':18082,'publicPort':public_port,'protocol':native_allocation['protocol']}
                            if status != 202 or json.loads(body) != expected: raise ValueError('owner API reservation differs')
                            return
                        encoded = json.dumps(native_allocation, separators=(',',':'))
                        redis_command('EVAL', "redis.call('HSET',KEYS[1],ARGV[1],ARGV[3]);redis.call('HSET',KEYS[2],ARGV[2],ARGV[3]);return 1", 2,
                            prefix + ':public-ports', prefix + ':ports:' + sandbox, public_port, 18082, encoded)
                    def native_resource_sample(phase):
                        if not args.native_resource_samples:return
                        if native_process.poll() is not None:raise ValueError('gateway exited before memory sample')
                        values={}
                        for line in Path(f'/proc/{native_process.pid}/smaps_rollup').read_text().splitlines():
                            fields=line.split()
                            if len(fields)==3 and fields[0].rstrip(':') in ('Rss','Pss','Private_Clean','Private_Dirty','Anonymous'):
                                if fields[2]!='kB':raise ValueError('unexpected memory unit')
                                values[fields[0].rstrip(':')]=int(fields[1])
                        if set(values)!=set(('Rss','Pss','Private_Clean','Private_Dirty','Anonymous')):raise ValueError('incomplete gateway memory sample')
                        native_resources.append({'phase':phase,'gateway_pid':native_process.pid,'rss_kib':values['Rss'],'pss_kib':values['Pss'],
                            'private_kib':values['Private_Clean']+values['Private_Dirty'],'anonymous_kib':values['Anonymous']})
                    def gateway_command():
                        return [str(args.native_gateway),'--bind-ip',peer_host,'--namespace',namespace,
                            '--mtls-ca',str(root/'ca.pem'),'--mtls-cert',str(root/'control.pem'),'--mtls-key',str(root/'control.key'),
                            '--max-ports',str(32 if args.native_capacity else 8),'--max-sessions',str(args.peer_count+(34 if args.native_tcp_capacity else 18 if args.native_capacity else 2)),
                            '--udp-peers',str(args.peer_count),'--poll-ms','20']
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
                    if args.owner_port_api:
                        for key in [other_key,observer_key,inventory_key,unassigned_key,control_api_key]:
                            for method,path,body in [('PUT',public_api_path+'/18082',{'protocol':'both'}),('GET',public_api_path,None),('DELETE',public_api_path+'/18082',None)]:
                                if args.owner_port_cli:
                                    public_cli({'PUT':'expose','GET':'list','DELETE':'remove'}[method],credential=key,
                                        protocol='both' if method=='PUT' else None,expect_status=403)
                                if status_api(method,path,key,body,{'x-hv2-sandbox-owner':'owned-principal-a'})[0] != 403:
                                    raise ValueError('public port API accepted wrong observer or unassigned creator')
                        if redis_command('HLEN',prefix+':public-ports') != '0': raise ValueError('refused public port requests wrote allocations')
                        legacy = api('POST','/sandboxes',{'templateID':'base','timeout':60},{'X-Api-Key':control_api_key})['sandboxID']
                        owned_children.append(legacy)
                        if json.loads(redis_command('GET',prefix+':sandbox:'+legacy)).get('owner_id') is not None: raise ValueError('legacy fixture was not ownerless')
                        for method,path,body in [('PUT',f'/sandboxes/{legacy}/public-ports/18082',{}),('GET',f'/sandboxes/{legacy}/public-ports',None),('DELETE',f'/sandboxes/{legacy}/public-ports/18082',None)]:
                            if args.owner_port_cli:
                                public_cli({'PUT':'expose','GET':'list','DELETE':'remove'}[method],target=legacy,
                                    protocol='both' if method=='PUT' else None,expect_status=403)
                            if status_api(method,path,token,body)[0] != 403: raise ValueError('public port API adopted an ownerless VM')
                        if args.owner_adoption:
                            def adopt_cli(target,credential=control_api_key,principal='owned-principal-a',expect_status=None):
                                command=[str(args.cli),'sandbox','vm','--endpoint',base,'--api-ca-cert',str(root/'ca.pem'),
                                    'adopt-owner',target,'--principal-id',principal]
                                result=subprocess.run(command,env=dict(os.environ,HV2_API_KEY=credential),capture_output=True,timeout=15)
                                if credential.encode() in result.stdout+result.stderr: raise ValueError('adoption CLI exposed API credential')
                                if expect_status is not None:
                                    if result.returncode==0 or str(expect_status).encode() not in result.stderr: raise ValueError('adoption CLI refusal differs')
                                elif result.returncode!=0 or result.stdout.strip(): raise ValueError('adoption CLI success or empty output differs')
                            adopt_cli(legacy,observer_key,expect_status=403)
                            adopt_cli(legacy,inventory_key,expect_status=403)
                            for body in [{'principalId':'invalid owner'},{'principalId':'owned-principal-a','extra':True}]:
                                if status_api('POST',f'/sandboxes/{legacy}/owner',control_api_key,body)[0]!=400: raise ValueError('adoption body validation differs')
                            adopt_cli(legacy);verify_owner(legacy,'running-adoption')
                            adopt_cli(legacy);adopt_cli(legacy,principal='owned-principal-b',expect_status=409)
                            public_cli('expose',target=legacy,protocol='both')
                            public_cli('list',credential=other_key,target=legacy,expect_status=403)
                            public_cli('remove',target=legacy)
                            results=api('POST',f'/sandboxes/{legacy}/fork',{'count':1,'timeout':60})
                            children=[row['sandbox']['sandboxID'] for row in results if 'sandbox' in row]
                            if len(children)!=1: raise ValueError('adopted source fork failed')
                            owned_children.extend(children)
                            for child in children:
                                verify_owner(child,'adopted-fork-inherits-owner');api('DELETE',f'/sandboxes/{child}');owned_children.remove(child)
                            api('POST',f'/sandboxes/{legacy}/pause',{});verify_owner(legacy,'adopted-source-paused')
                            api('POST',f'/sandboxes/{legacy}/resume',{});verify_owner(legacy,'adopted-source-resumed')
                            checks.append('Administrator CLI running adoption replay transfer refusal owner-only ports fork and pause resume retain assigned owner')
                            paused_legacy=api('POST','/sandboxes',{'templateID':'base','timeout':60},{'X-Api-Key':control_api_key})['sandboxID']
                            owned_children.append(paused_legacy);api('POST',f'/sandboxes/{paused_legacy}/pause',{})
                            adopt_cli(paused_legacy);verify_owner(paused_legacy,'paused-adoption')
                            metadata=json.loads((root/'snapshots'/'paused'/f'{paused_legacy}.json').read_text())
                            if metadata['record'].get('owner_id')!='owned-principal-a': raise ValueError('paused metadata lost adopted owner')
                            adopt_cli(paused_legacy);api('POST',f'/sandboxes/{paused_legacy}/resume',{})
                            verify_owner(paused_legacy,'paused-adoption-resumed')
                            api('DELETE',f'/sandboxes/{paused_legacy}');owned_children.remove(paused_legacy)
                            checks.append('Administrator paused adoption updates shared snapshot metadata and resume preserves assigned owner')
                            if args.owner_adoption_cross_node:
                                secondary_port,secondary_proxy=port(),port()
                                secondary_id='owned-adoption-secondary'
                                secondary_process=subprocess.Popen([str(args.daemon),'--port',str(secondary_port),'--proxy-port',str(secondary_proxy),
                                    '--cluster-store',store_url,'--cluster-namespace',namespace,'--node-id',secondary_id,
                                    '--advertise-api',f'https://127.0.0.1:{secondary_port}','--advertise-proxy',f'127.0.0.1:{secondary_proxy}',
                                    '--snapshot-store',str(root/'snapshots')]+node_tls_arguments,stdout=log,stderr=log,
                                    env=dict(os.environ,HV2_KERNEL=str(args.kernel),HV2_INITRD=str(args.initrd),HV2_CLUSTER_TOKEN=cluster_token,RUST_LOG='info'))
                                services.append(secondary_process)
                                deadline=time.monotonic()+30
                                while redis_command('EXISTS',prefix+':node:'+secondary_id)!='1':
                                    if secondary_process.poll() is not None or time.monotonic()>deadline: raise TimeoutError('secondary adoption node readiness')
                                    time.sleep(.05)
                                def direct_node(target_port,method,path,body):
                                    payload=json.dumps(body).encode()
                                    with socket.create_connection(('127.0.0.1',target_port),timeout=10) as raw:
                                        with node_context.wrap_socket(raw,server_hostname='hv2-node') as client:
                                            client.settimeout(30)
                                            client.sendall((f'{method} {path} HTTP/1.1\r\nHost: hv2-node\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {len(payload)}\r\nX-Hv2-Cluster-Token: {cluster_token}\r\n\r\n').encode()+payload)
                                            response=http.client.HTTPResponse(client);response.begin()
                                            data=response.read(1048577)
                                            if len(data)>1048576: raise ValueError('migration response exceeds bound')
                                            return response.status,data
                                # The source is explicitly created on node one; the control scheduler now has two nodes.
                                status,data=direct_node(node_port,'POST','/sandboxes',{'templateID':'base','timeout':120})
                                if status!=201: raise ValueError('migration legacy source creation failed')
                                migrating=json.loads(data)['sandboxID'];owned_children.append(migrating)
                                launch=api('POST',f'/sandboxes/{migrating}/exec',{'cmd':'/bin/tcp-fixture >/tmp/migration-tcp.log 2>&1 & /bin/hm-udp-echo --port 18082 >/tmp/migration-udp.log 2>&1 &','timeout_secs':5})
                                if launch['exit_code']!=0: raise ValueError('migration guest fixtures failed')
                                api('POST',f'/sandboxes/{migrating}/pause',{})
                                barrier=threading.Barrier(2,timeout=15)
                                def competing_node(operation):
                                    barrier.wait()
                                    if operation=='adopt': return direct_node(secondary_port,'POST',f'/sandboxes/{migrating}/owner',{'principalId':'owned-principal-a'})[0]
                                    return direct_node(node_port,'POST',f'/sandboxes/{migrating}/resume',{})[0]
                                with ThreadPoolExecutor(max_workers=2) as executor:
                                    race=list(executor.map(competing_node,['adopt','resume']))
                                if race[0] not in [204,409] or race[1] not in [200,201,404,409] or not (race[0]==204 or race[1] in [200,201]): raise ValueError('cross-node claim race differs')
                                # Exercise the adopter-first ordering by temporarily pausing only writes
                                # on this owned Redis. The node holds the shared description claim while
                                # its atomic adoption write waits; node one must refuse a concurrent resume.
                                status,data=direct_node(node_port,'POST','/sandboxes',{'templateID':'base','timeout':120})
                                if status!=201: raise ValueError('held adoption source creation failed')
                                held_id=json.loads(data)['sandboxID'];owned_children.append(held_id)
                                api('POST',f'/sandboxes/{held_id}/pause',{})
                                redis_command('CLIENT','PAUSE','300','WRITE')
                                with ThreadPoolExecutor(max_workers=1) as executor:
                                    pending=executor.submit(direct_node,secondary_port,'POST',f'/sandboxes/{held_id}/owner',{'principalId':'owned-principal-a'})
                                    claim_path=root/'snapshots'/'paused'/f'{held_id}.json.claimed-{secondary_id}'
                                    deadline=time.monotonic()+.2
                                    while not claim_path.exists():
                                        if pending.done() or time.monotonic()>deadline: raise TimeoutError('controlled adoption did not hold paused claim')
                                        time.sleep(.002)
                                    if direct_node(node_port,'POST',f'/sandboxes/{held_id}/resume',{})[0]!=404: raise ValueError('resume bypassed held cross-node adoption claim')
                                    held_status=pending.result(timeout=10)[0]
                                if held_status!=204: raise ValueError('held adoption did not commit')
                                verify_owner(held_id,'cross-node-adopter-first')
                                if json.loads((root/'snapshots'/'paused'/f'{held_id}.json').read_text())['record'].get('owner_id')!='owned-principal-a': raise ValueError('held adoption metadata owner differs')
                                api('DELETE',f'/sandboxes/{held_id}');owned_children.remove(held_id)
                                record=json.loads(redis_command('GET',prefix+':sandbox:'+migrating))
                                if not record.get('paused'): api('POST',f'/sandboxes/{migrating}/pause',{})
                                if direct_node(secondary_port,'POST',f'/sandboxes/{migrating}/owner',{'principalId':'owned-principal-a'})[0]!=204: raise ValueError('secondary paused adoption recovery failed')
                                verify_owner(migrating,'cross-node-paused-adopted')
                                status,_=direct_node(secondary_port,'POST',f'/sandboxes/{migrating}/resume',{})
                                if status not in [200,201]: raise ValueError('secondary migration resume failed')
                                record=json.loads(redis_command('GET',prefix+':sandbox:'+migrating))
                                if record['node_id']!=secondary_id or record.get('paused') or record.get('owner_id')!='owned-principal-a': raise ValueError('migration record node or owner differs')
                                verify_owner(migrating,'cross-node-resumed')
                                result=api('POST',f'/sandboxes/{migrating}/exec',{'cmd':'printf migrated-owned','timeout_secs':5})
                                if result['exit_code']!=0 or result['stdout']!='migrated-owned': raise ValueError('migrated guest execution differs')
                                public_cli('expose',target=migrating,protocol='both')
                                native_process=subprocess.Popen(gateway_command(),env=dict(os.environ,HV2_STORE_URL=store_url,HV2_CLUSTER_TOKEN=cluster_token),stdout=log,stderr=log)
                                services.append(native_process)
                                migrated_tcp=native_connect();native_tcp_echo(migrated_tcp,bytes(range(256))*4096)
                                with socket.socket(peer_family,socket.SOCK_DGRAM) as migrated_udp:
                                    migrated_udp.bind((peer_host,0));migrated_udp.settimeout(.5)
                                    for payload in [b'',b'migrated\x00\xff',bytes(range(251))*260]:native_udp_echo(migrated_udp,payload)
                                if args.native_port_migration:
                                    allocation=redis_command('HGET',prefix+':public-ports',public_port)
                                    rows=public_cli('list',target=migrating)
                                    expected=[{'machinePort':18082,'publicPort':public_port,'protocol':'both'}]
                                    if rows!=expected: raise ValueError('pre-migration public reservation differs')
                                    gateway_pid=native_process.pid
                                    api('POST',f'/sandboxes/{migrating}/pause',{})
                                    closed(migrated_tcp);native_absent()
                                    deadline=time.monotonic()+5
                                    while True:
                                        with socket.socket(peer_family,socket.SOCK_DGRAM) as probe:
                                            try:probe.bind(native_address);break
                                            except OSError:
                                                if time.monotonic()>deadline: raise TimeoutError('migration pause retained UDP listener')
                                        time.sleep(.02)
                                    if public_cli('list',target=migrating)!=expected or redis_command('HGET',prefix+':public-ports',public_port)!=allocation: raise ValueError('pause changed migration reservation')
                                    checks.append('Published migrated VM pause closes TCP unbinds UDP and preserves byte-identical both-protocol allocation')
                                    status,_=direct_node(node_port,'POST',f'/sandboxes/{migrating}/resume',{})
                                    if status not in [200,201]: raise ValueError('return migration resume failed')
                                    record=json.loads(redis_command('GET',prefix+':sandbox:'+migrating))
                                    if record['node_id']!='owned-udp-node' or record.get('paused'): raise ValueError('return migration node state differs')
                                    verify_owner(migrating,'cross-node-return-resumed')
                                    if public_cli('list',target=migrating)!=expected or redis_command('HGET',prefix+':public-ports',public_port)!=allocation: raise ValueError('return migration changed reserved port')
                                    if native_process.pid!=gateway_pid or native_process.poll() is not None: raise ValueError('gateway restarted during migration')
                                    migrated_tcp=native_connect();native_tcp_echo(migrated_tcp,bytes(range(256))*4096)
                                    with socket.socket(peer_family,socket.SOCK_DGRAM) as returned_udp:
                                        returned_udp.bind((peer_host,0));returned_udp.settimeout(.5)
                                        for payload in [b'',b'returned\x00\xff',bytes(range(251))*260]:native_udp_echo(returned_udp,payload)
                                    port_migration_report={'public_port':public_port,'protocol':'both','node_before':secondary_id,'node_after':'owned-udp-node',
                                        'reservation_byte_identical':True,'gateway_pid_unchanged':True,'tcp_bytes_after':1048576,'udp_payload_bytes_after':[0,10,65260]}
                                    checks.append('Same native gateway republishes unchanged both-protocol public port after return to first KVM node and verifies actual TCP UDP')
                                results=api('POST',f'/sandboxes/{migrating}/fork',{'count':1,'timeout':60})
                                children=[row['sandbox']['sandboxID'] for row in results if 'sandbox' in row]
                                if len(children)!=1: raise ValueError('migrated source fork failed')
                                owned_children.extend(children)
                                for child in children:
                                    verify_owner(child,'migrated-fork-owner');api('DELETE',f'/sandboxes/{child}');owned_children.remove(child)
                                api('DELETE',f'/sandboxes/{migrating}');owned_children.remove(migrating)
                                closed(migrated_tcp);native_absent()
                                if redis_command('HLEN',prefix+':public-ports')!='0': raise ValueError('migration deletion left allocations')
                                native_process.send_signal(signal.SIGTERM)
                                if native_process.wait(timeout=8)!=0: raise ValueError('migration gateway did not stop gracefully')
                                secondary_process.terminate()
                                if secondary_process.wait(timeout=10)!=0: raise ValueError('secondary node did not stop gracefully')
                                migration_report={'adoption_race_status':race[0],'resume_race_status':race[1],
                                    'held_adoption_status':held_status,'resume_during_held_claim_status':404,
                                    'resumed_node':secondary_id,'tcp_verified_bytes':1048576,'udp_payload_bytes':[0,10,65260],
                                    'fork_owner_verified':True,'reservation_delete_verified':True,'secondary_node_reaped':True,'migration_gateway_reaped':True}
                                checks.append('Two KVM nodes serialize paused adoption resume race and migrated guest preserves owner execution TCP UDP fork and allocation cleanup')

                        api('DELETE',f'/sandboxes/{legacy}'); owned_children.remove(legacy)
                        checks.append('Public port first reservation listing and removal refuse wrong observer inventory unassigned legacy principals and ownerless VMs')
                    reserve_native()
                    native_process = subprocess.Popen(gateway_command(),
                        env=dict(os.environ,HV2_STORE_URL=store_url,HV2_CLUSTER_TOKEN=cluster_token),stdout=log,stderr=log)
                    services.append(native_process)
                    native_tcp = native_connect(); native_tcp_echo(native_tcp, bytes(range(256))*4096)
                    for index in range(args.peer_count):
                        peer = socket.socket(peer_family,socket.SOCK_DGRAM); peer.bind((peer_host,0)); peer.settimeout(.5); native_peers.append(peer)
                        for payload in [b'',bytes([index+1,0,255,13]),bytes([index+1])*65507]: native_udp_echo(peer,payload)
                    checks.append(f'Native gateway real KVM TCP preserves 1 MiB and {args.peer_count} UDP peers preserve empty binary and maximum datagrams on one public port')
                    native_resource_sample('after_maximum_udp')
                    if args.native_comparison_samples:
                        cli_process = subprocess.Popen([str(args.cli), 'sandbox', 'vm', '--endpoint', base,
                            '--api-ca-cert', str(root/'ca.pem'), 'udp', sandbox, '--port', '18082',
                            '--max-peers', str(args.peer_count)] + (['--listen', '[::1]:0'] if args.local_ipv6 else []),
                            stdout=subprocess.PIPE, stderr=log, env=dict(os.environ, HV2_API_KEY=token))
                        if not select.select([cli_process.stdout], [], [], 5)[0]: raise TimeoutError('comparison CLI readiness')
                        announced = json.loads(cli_process.stdout.readline())
                        compare_host, compare_port = announced['listen'].rsplit(':', 1)
                        cli_address = (compare_host.strip('[]'), int(compare_port))
                        for _ in range(args.peer_count):
                            peer = socket.socket(peer_family, socket.SOCK_DGRAM)
                            comparison_peers.append(peer); peer.bind((peer_host, 0)); peer.settimeout(5)
                        for peer in native_peers: peer.settimeout(5)
                        for block, path in enumerate(('cli', 'native', 'native', 'cli')):
                            active_peers = comparison_peers if path == 'cli' else native_peers
                            address = cli_address if path == 'cli' else native_address
                            barrier = threading.Barrier(args.peer_count, timeout=15)
                            def matched_peer(index):
                                samples = []
                                tail = bytes(n % 251 for n in range(args.native_comparison_bytes - 6))
                                for sequence in range(args.native_comparison_samples + 10):
                                    if sequence == 10:
                                        barrier.wait(); measured_started = time.monotonic()
                                    payload = bytes([block, index]) + sequence.to_bytes(4, 'big') + tail
                                    started = time.perf_counter_ns()
                                    active_peers[index].sendto(payload, address)
                                    received, source = active_peers[index].recvfrom(65508)
                                    if received != payload or (source[0], source[1]) != address:
                                        raise ValueError('matched comparison payload/source mismatch')
                                    elapsed = (time.perf_counter_ns() - started) / 1e6
                                    if sequence >= 10: samples.append(elapsed)
                                duration = time.monotonic() - measured_started
                                ordered = sorted(samples)
                                return {'peer': index, 'samples_ms': samples, 'measured_wall_seconds': duration,
                                    'p50_ms': ordered[math.ceil(len(samples)*.50)-1],
                                    'p95_ms': ordered[math.ceil(len(samples)*.95)-1],
                                    'p99_ms': ordered[math.ceil(len(samples)*.99)-1]}
                            with ThreadPoolExecutor(max_workers=args.peer_count) as executor:
                                results = list(executor.map(matched_peer, range(args.peer_count)))
                            native_comparison.append({'block': block, 'path': path, 'guest_port': 18082,
                                'payload_bytes': args.native_comparison_bytes, 'warmups_per_peer': 10,
                                'samples_per_peer': args.native_comparison_samples, 'peers': results,
                                'roundtrips_per_second': args.peer_count * args.native_comparison_samples /
                                    max(item['measured_wall_seconds'] for item in results)})
                        cli_process.send_signal(signal.SIGINT)
                        if cli_process.wait(timeout=8) != 0: raise ValueError('comparison CLI shutdown failed')
                        for peer in comparison_peers: peer.close()
                        checks.append('Matched CLI/native/native/CLI blocks verify tagged datagrams against the same actual guest UDP port')
                        native_resource_sample('after_abba')
                    if args.owner_context:
                        results = api('POST', f'/sandboxes/{sandbox}/fork', {'count':2,'timeout':60})
                        for result in results:
                            if 'sandbox' in result: owned_children.append(result['sandbox']['sandboxID'])
                        if len(owned_children) != 2: raise ValueError('owned fork fixture did not create two children')
                        for child in list(owned_children):
                            verify_owner(child, 'fork-inherits-source-owner')
                            if redis_command('HLEN', prefix+':ports:'+child) != '0': raise ValueError('fork inherited native port allocations')
                            api('DELETE', f'/sandboxes/{child}'); owned_children.remove(child)
                        if redis_command('HLEN', prefix+':public-ports') != '1' or json.loads(redis_command('HGET', prefix+':public-ports', public_port)) != native_allocation: raise ValueError('fork changed source native allocation')
                        checks.append('Authenticated policy creator overrides forged client context; two real KVM forks inherit owner without inheriting the source native allocation')
                    native_allocation['protocol'] = 'udp'; reserve_native(); closed(native_tcp); native_absent()
                    native_udp_echo(native_peers[0],b'native-protocol-update')
                    native_allocation['protocol'] = 'both'; reserve_native(); native_tcp = native_connect(); native_tcp_echo(native_tcp,b'native-both-restored')
                    checks.append('Native real guest protocol updates retain public port and close old TCP')
                    if args.owner_port_api:
                        if args.owner_port_cli:
                            status,body=200,json.dumps(public_cli('list')).encode()
                        else: status,body=status_api('GET',public_api_path,token)
                        expected=[{'machinePort':18082,'publicPort':public_port,'protocol':'both'}]
                        if status!=200 or json.loads(body)!=expected: raise ValueError('owned listing differs')
                        if not args.native_capacity and status_api('PUT',public_api_path+'/18083',token,{})[0]!=503: raise ValueError('owned pool was not bounded')
                        if status_api('PUT',public_api_path+'/0',token,{})[0]!=400: raise ValueError('owner API accepted zero port')
                        old_token=token; token=secrets.token_hex(32);fixture_credentials.append(token)
                        owner_policies[0]['sha256']=hashlib.sha256(token.encode()).hexdigest()
                        policy_path.write_text(json.dumps(owner_policies));control.send_signal(signal.SIGHUP)
                        deadline=time.monotonic()+8
                        while status_api('GET',public_api_path,token)[0]!=200:
                            if time.monotonic()>deadline: raise TimeoutError('owner key reload')
                            time.sleep(.02)
                        if status_api('GET',public_api_path,old_token)[0]!=401: raise ValueError('revoked owner key still authorized')
                        if args.owner_port_cli:
                            public_cli('list',credential=old_token,expect_status=401)
                            if public_cli('list')!=expected: raise ValueError('rotated CLI owner listing differs')
                        verify_owner(sandbox,'key-rotated')
                        if json.loads(redis_command('HGET',prefix+':public-ports',public_port))!=native_allocation: raise ValueError('key rotation changed allocation')
                        checks.append('Owner API listing bounded pool invalid port and actual key rotation preserve VM ownership and allocation')
                        if args.owner_port_cli:
                            if public_cli('remove') is not None: raise ValueError('CLI removal printed an unexpected body')
                        elif status_api('DELETE',public_api_path+'/18082',token)[0]!=204: raise ValueError('owner reservation deletion failed')
                        closed(native_tcp);native_absent()
                        if status_api('GET',public_api_path,token)!=(200,b'[]'): raise ValueError('owner removal left visible reservations')
                        if args.owner_port_cli:
                            if public_cli('list')!=[]: raise ValueError('CLI removal left reservations')
                            public_cli('remove',expect_status=404)
                            default=public_cli('expose')
                            if default!={'machinePort':18082,'publicPort':public_port,'protocol':'tcp'}: raise ValueError('CLI default TCP differs')
                            native_tcp=native_connect();native_tcp_echo(native_tcp,b'owner-cli-default-tcp')
                            reserve_native();closed(native_tcp)
                        else: reserve_native()
                        native_tcp=native_connect();native_tcp_echo(native_tcp,b'owner-api-reexpose')
                        native_udp_echo(native_peers[0],b'owner-api-udp-reexpose')
                        checks.append('Owner API removal closes live native sessions and reexposure publishes actual TCP UDP on reserved port')
                        if args.owner_port_cli: checks.append('Shipped CLI exposes lists removes defaults to TCP updates to both and enforces owner and rotated credential refusals through real KVM')
                    if args.native_capacity:
                        public_cli('remove');closed(native_tcp);native_absent()
                        command='; '.join(f'/bin/hm-udp-echo --port {19000+i} --tag-port >/tmp/capacity-{i}.log 2>&1 &' for i in range(16))
                        # Background commands need newlines, not a second separator after '&'.
                        command=command.replace('&; ','&\n')
                        result=api('POST',f'/sandboxes/{sandbox}/exec',{'cmd':command,'timeout_secs':5})
                        if result['exit_code']!=0: raise ValueError('capacity guest service launch failed')
                        for index in range(16):
                            row=public_cli('expose',protocol='both' if args.native_tcp_capacity else 'udp',destination=19000+index)
                            if row['machinePort']!=19000+index or row['protocol']!=('both' if args.native_tcp_capacity else 'udp'): raise ValueError('capacity allocation differs')
                            capacity_rows.append(row)
                            peer=socket.socket(peer_family,socket.SOCK_DGRAM);capacity_peers.append(peer)
                            peer.bind((peer_host,0));peer.settimeout(.5)
                        if len({row['publicPort'] for row in capacity_rows})!=16: raise ValueError('capacity ports are not unique')
                        public_cli('expose',protocol='udp',destination=20000,expect_status=409)
                        if public_cli('list')!=capacity_rows: raise ValueError('capacity listing differs')
                        def capacity_transfer(index,payload,retry=False):
                            row=capacity_rows[index];address=(peer_host,row['publicPort'])
                            expected=row['machinePort'].to_bytes(2,'big')+payload
                            deadline=time.monotonic()+8
                            while True:
                                capacity_peers[index].sendto(payload,address)
                                try:
                                    received,source=capacity_peers[index].recvfrom(65508)
                                    if received!=expected or (source[0],source[1])!=address: raise ValueError('capacity guest destination payload or source differs')
                                    return
                                except socket.timeout:
                                    if not retry or time.monotonic()>deadline: raise
                        for index in range(16): capacity_transfer(index,b'ready'+bytes([index]),True)
                        barrier=threading.Barrier(16,timeout=15)
                        def capacity_worker(index):
                            capacity_peers[index].settimeout(5);barrier.wait()
                            for sequence in range(100):
                                payload=bytes([index])+sequence.to_bytes(4,'big')+bytes(n%251 for n in range(507))
                                capacity_transfer(index,payload)
                            return 100
                        with ThreadPoolExecutor(max_workers=16) as executor:
                            counts=list(executor.map(capacity_worker,range(16)))
                        capacity_report={'simultaneous_udp_ports':16,'roundtrips_per_port':counts,'payload_bytes':512,
                            'destination_tag_bytes':2,'published_rows':list(capacity_rows),'seventeenth_refused_status':409}
                        checks.append('Sixteen simultaneous native UDP ports verify 1600 concurrent tagged guest replies and refuse a seventeenth reservation')
                        if args.native_tcp_capacity:
                            def capacity_tcp_connect(row):
                                deadline=time.monotonic()+8
                                while True:
                                    client=None
                                    try:
                                        client=socket.create_connection((peer_host,row['publicPort']),timeout=.5)
                                        client.settimeout(5)
                                        if exact(client,2)!=row['machinePort'].to_bytes(2,'big'): raise ValueError('TCP guest destination tag differs')
                                        streams.append(client);return client
                                    except OSError:
                                        if client is not None: client.close()
                                        if time.monotonic()>deadline: raise
                                        time.sleep(.02)
                            for row in capacity_rows: capacity_tcp.append(capacity_tcp_connect(row))
                            tcp_barrier=threading.Barrier(16,timeout=15)
                            def capacity_tcp_worker(index):
                                client=capacity_tcp[index];tcp_barrier.wait()
                                for sequence in range(100):
                                    payload=bytes([index])+sequence.to_bytes(4,'big')+bytes(n%251 for n in range(4091))
                                    client.sendall(payload)
                                    if exact(client,len(payload))!=payload: raise ValueError('TCP capacity payload differs')
                                return 100
                            with ThreadPoolExecutor(max_workers=16) as executor:
                                tcp_counts=list(executor.map(capacity_tcp_worker,range(16)))
                            capacity_report['simultaneous_tcp_ports']=16
                            capacity_report['tcp_roundtrips_per_port']=tcp_counts
                            capacity_report['tcp_payload_bytes']=4096
                            checks.append('Sixteen simultaneous native TCP connections verify 1600 distinct-destination 4096-byte round trips alongside live UDP peers')
                            if args.native_mixed_capacity:
                                mixed_barrier=threading.Barrier(32,timeout=15)
                                def mixed_worker(worker):
                                    protocol,index=worker
                                    mixed_barrier.wait()
                                    for sequence in range(100):
                                        size=512 if protocol=='udp' else 4096
                                        payload=bytes([index])+sequence.to_bytes(4,'big')+bytes(n%251 for n in range(size-5))
                                        if protocol=='udp': capacity_transfer(index,payload)
                                        else:
                                            client=capacity_tcp[index];client.sendall(payload)
                                            if exact(client,len(payload))!=payload: raise ValueError('mixed TCP capacity payload differs')
                                    return {'protocol':protocol,'destination':capacity_rows[index]['machinePort'],'roundtrips':100}
                                workers=[(protocol,index) for protocol in ('tcp','udp') for index in range(16)]
                                with ThreadPoolExecutor(max_workers=32) as executor:
                                    mixed_counts=list(executor.map(mixed_worker,workers))
                                capacity_report['mixed_traffic']={'workers':32,'roundtrips':mixed_counts,
                                    'tcp_payload_bytes':4096,'udp_payload_bytes':512,'retries':0}
                                checks.append('Thirty-two mixed TCP UDP workers verify 3200 simultaneous distinct-destination exchanges across sixteen both-protocol publications')

                        # Restore the primary both-protocol destination and keep fifteen tagged ports live.
                        public_cli('remove',destination=capacity_rows[0]['machinePort'])
                        capacity_peers.pop(0).close();capacity_rows.pop(0)
                        if args.native_tcp_capacity: closed(capacity_tcp.pop(0))
                        reserve_native();native_tcp=native_connect();native_tcp_echo(native_tcp,b'capacity-primary-restored')
                        native_udp_echo(native_peers[0],b'capacity-primary-udp')
                        capacity_snapshot=public_cli('list')
                        if len(capacity_snapshot)!=16: raise ValueError('capacity lifecycle inventory differs')
                        def capacity_absent():
                            deadline=time.monotonic()+5
                            for row in capacity_snapshot:
                                while True:
                                    with socket.socket(peer_family,socket.SOCK_DGRAM) as probe:
                                        try: probe.bind((peer_host,row['publicPort']));break
                                        except OSError:
                                            if time.monotonic()>deadline: raise TimeoutError('capacity UDP listener remained bound')
                                    time.sleep(.02)



                stream, status = tunnel(sandbox)
                streams.append(stream)
                if status != 101: raise ValueError('pre-pause session setup failed')
                roundtrip(stream, b'live-before-pause')
                api('POST', f'/sandboxes/{sandbox}/pause', {})
                closed(stream)
                checks.append('Pause closes existing UDP session')
                if args.owner_context: verify_owner(sandbox, 'paused')
                if args.native_gateway:
                    closed(native_tcp); native_absent()
                    if json.loads(redis_command('HGET', prefix+':public-ports', public_port)) != native_allocation: raise ValueError('pause changed native reservation')
                    checks.append('Real guest pause closes native sessions and preserves exact public reservation')
                    if args.native_capacity:
                        capacity_absent()
                        if args.native_tcp_capacity:
                            for client in capacity_tcp: closed(client)
                            checks.append('Pause closes all fifteen tagged TCP sessions and primary TCP session')
                        if public_cli('list')!=capacity_snapshot: raise ValueError('pause changed capacity reservations')
                        checks.append('Pause unbinds all sixteen UDP listeners and preserves exact owner reservations')
                if args.resume_publication_fault:
                    def fault_node(method, suffix, credential=cluster_token):
                        connection=http.client.HTTPSConnection('127.0.0.1',node_port,context=node_context,timeout=30)
                        # Certificate uses hv2-node: wrap the actual connected socket explicitly.
                        connection.connect = lambda: setattr(connection,'sock',node_context.wrap_socket(socket.create_connection(('127.0.0.1',node_port),timeout=30),server_hostname='hv2-node'))
                        try:
                            connection.request(method,f'/sandboxes/{sandbox}'+suffix,body=b'{}',headers={'Content-Type':'application/json','X-Hv2-Cluster-Token':credential})
                            response=connection.getresponse();body=response.read();return response.status,body
                        finally:connection.close()
                    original=redis_command('GET',prefix+':sandbox:'+sandbox)
                    redis_command('ACL','SETUSER','default','-'+args.resume_publication_fault_command.lower())
                    try:
                        if fault_node('POST','/resume')[0]!=503:raise ValueError('failed resume publication did not report uncertainty')
                        after=json.loads(redis_command('GET',prefix+':sandbox:'+sandbox))
                        if args.resume_publication_fault_command=='SET':
                            if redis_command('GET',prefix+':sandbox:'+sandbox)!=original:raise ValueError('refused record write changed paused record')
                        elif after.get('paused') or after.get('node_id')!='owned-udp-node' or after.get('owner_id')!='owned-principal-a':
                            raise ValueError('event refusal did not preserve committed running record')
                        if fault_node('POST','/pause')[0]!=409:raise ValueError('uncertain resume accepted pause mutation')
                        if fault_node('POST','/registration/reconcile','wrong-cluster-token')[0]!=401:raise ValueError('reconciliation accepted wrong cluster credential')
                    finally:
                        if not args.automatic_registration_recovery:redis_command('ACL','SETUSER','default','+'+args.resume_publication_fault_command.lower())
                    if args.recovery_cli:
                        def reconcile_cli(key, node='owned-udp-node'):
                            result=subprocess.run([str(args.cli),'sandbox','vm','--endpoint',base,'--api-ca-cert',str(root/'ca.pem'),
                                'reconcile-registration',sandbox,'--node-id',node],env=dict(os.environ,HV2_API_KEY=key),capture_output=True,timeout=30)
                            if any(secret.encode() in result.stdout+result.stderr for secret in (token,old_token,cluster_token,other_key,observer_key,inventory_key,unassigned_key,sandboxes_key,control_api_key)):raise ValueError('recovery CLI leaked API or cluster credentials')
                            return result
                        for key in (observer_key,inventory_key,sandboxes_key):
                            if reconcile_cli(key).returncode==0:raise ValueError('nonadministrator recovered registration')
                        if reconcile_cli(token,'unregistered-node').returncode==0:raise ValueError('recovery selected an unregistered node')
                        credential=control_api_key if args.resume_publication_fault_command=='SET' else token
                        if args.pending_discovery:require_pending({sandbox})
                        if args.automatic_registration_recovery:
                            time.sleep(1.2)
                            require_pending({sandbox})
                            if args.registration_recovery_timeout:redis_command('CLIENT','PAUSE','8000','WRITE')
                            redis_command('ACL','SETUSER','default','+'+args.resume_publication_fault_command.lower())
                            if args.registration_recovery_timeout:
                                try:
                                    deadline=time.monotonic()+7
                                    while sum('registration reconciliation remains uncertain' in line and 'cluster store: timed out' in line and sandbox in line for line in (args.output/'daemon.log').read_text(errors='replace').splitlines())<2:
                                        if time.monotonic()>deadline:raise TimeoutError('worker did not retry after bounded store timeouts')
                                        time.sleep(.05)
                                    require_pending({sandbox})
                                    checks.append('Repeated bounded store timeouts retain pending '+('initial' if sandbox==preserved else 'resumed')+' KVM guest during owned Redis write stall')
                                finally:redis_command('CLIENT','UNPAUSE')
                            deadline=time.monotonic()+20
                            while json.loads(creation_node('GET','/registrations/pending')[1])['registrations']:
                                if time.monotonic()>deadline:raise TimeoutError('automatic resumed registration recovery')
                                time.sleep(.05)
                            recovered_record=json.loads(redis_command('GET',prefix+':sandbox:'+sandbox))
                            if recovered_record['descriptor'].get('envdAccessToken')!=json.loads(original)['descriptor'].get('envdAccessToken'):raise ValueError('automatic recovery changed guest access token')
                            checks.append('Automatic worker recovers resumed KVM guest after Redis refusal without a reconciliation request and preserves its access token')
                        else:
                            recovered=reconcile_cli(credential)
                            if recovered.returncode!=0 or json.loads(recovered.stdout).get('sandboxID')!=sandbox:raise ValueError('administrator CLI recovery descriptor differs')
                            if json.loads(recovered.stdout).get('envdAccessToken')!=json.loads(original)['descriptor'].get('envdAccessToken'):raise ValueError('recovery changed guest access token')
                        if reconcile_cli(credential).returncode==0:raise ValueError('recovery replay ignored cleared pending marker')
                        if args.pending_discovery:
                            require_pending(set())
                            checks.append('Administrator CLI discovers uncertain resumed guest and recovery removes its pending entry')
                        checks.append(('Automatic recovery and shipped CLI refusals' if args.automatic_registration_recovery else 'Shipped CLI recovery')+' through explicit registered node: observer inventory sandbox scopes unknown node and cleared-marker replay refuse')
                    elif fault_node('POST','/registration/reconcile')[0]!=200:raise ValueError('restored store did not reconcile preserved running guest')
                    record=json.loads(redis_command('GET',prefix+':sandbox:'+sandbox))
                    if record.get('paused') or record.get('node_id')!='owned-udp-node' or record.get('owner_id')!='owned-principal-a':raise ValueError('reconciled resume record differs')
                    checks.append('Real Redis '+args.resume_publication_fault_command+' refusal preserves resumed KVM guest, blocks pause and unauthorized reconciliation, and recovery publishes running owner record')
                else:
                    api('POST', f'/sandboxes/{sandbox}/resume', {})
                resumed, status = tunnel(sandbox)
                streams.append(resumed)
                if status != 101: raise ValueError('resumed UDP setup failed')
                roundtrip(resumed, b'after-resume\x00\xff')
                checks.append('Resumed guest accepts a new UDP session')
                if args.owner_context:
                    verify_owner(sandbox, 'resumed')
                    checks.append('Actual KVM pause and resume preserve trusted owner identity')
                if args.native_gateway:
                    native_tcp = native_connect(); native_tcp_echo(native_tcp,b'native-after-resume')
                    native_udp_echo(native_peers[0],b'native-after-resume\x00\xff')
                    checks.append('Real guest resume restores native TCP and UDP on same reserved port')
                    if args.native_capacity:
                        if public_cli('list')!=capacity_snapshot: raise ValueError('resume changed capacity reservations')
                        for index in range(15): capacity_transfer(index,b'resumed'+bytes([index]),True)
                        checks.append('Resume restores all sixteen UDP destinations on unchanged reserved ports')
                        if args.native_tcp_capacity:
                            capacity_tcp.clear()
                            for row in capacity_rows:
                                client=capacity_tcp_connect(row);capacity_tcp.append(client);native_tcp_echo(client,b'resumed-tcp-capacity')
                            checks.append('Resume restores all sixteen TCP destinations on unchanged reserved ports')
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
                    if args.native_capacity:
                        capacity_absent()
                        checks.append('Deletion removes all sixteen allocations and unbinds their UDP listeners')
                        if args.native_tcp_capacity:
                            for client in capacity_tcp: closed(client)
                            checks.append('Deletion closes all sixteen native TCP destination sessions')
                    checks.append('Real guest deletion removes native allocation and closes native sessions; gateway stops gracefully')
            finally:
                for peer in native_peers + comparison_peers + capacity_peers: peer.close()
                for stream in streams: stream.close()
                if cli_process is not None and cli_process.poll() is None:
                    cli_process.kill(); cli_process.wait(timeout=5)
                if process is not None and process.poll() is None:
                    for child in owned_children:
                        try: api('DELETE', f'/sandboxes/{child}')
                        except OSError: pass
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
    logs = (args.output / 'daemon.log').read_bytes()
    if any(value.encode() in logs for value in fixture_credentials): raise ValueError('fixture credentials appeared in logs')
    for path, expected in inputs.items():
        if hashlib.sha256(Path(path).read_bytes()).hexdigest() != expected: raise ValueError('input changed')
    report = {'paused_state_mode': 'in-memory' if args.in_memory_pauses else 'shared-store', 'inputs_sha256': inputs, 'checks': checks, 'daemon_reaped': True, 'guests_remaining': 0,
        'control_and_redis_reaped': True, 'cli_reaped': True,
        'native_gateway_reaped': native_process is None or native_process.poll() is not None,
        'native_gateway_real_kvm': bool(args.native_gateway),
        'owner_public_port_api_real_kvm': args.owner_port_api,
        'owner_public_port_cli_real_kvm': args.owner_port_cli,
        'native_udp_capacity': capacity_report,
        'cross_node_adoption': migration_report,
        'native_port_migration': port_migration_report,
        'native_resources': native_resources,
        'secondary_node_reaped': secondary_process is None or secondary_process.poll() is not None,
        'trusted_owner_context': owner_checks,
        'guest_owner_from_store_at_creation': creation_owner,
        'native_cli_comparison': native_comparison,
        'host_build_profile_declared': args.host_build_profile,
        'guest_resources_from_inventory': guest_resources,
        'host_environment': {'kernel': platform.release(), 'machine': platform.machine(),
            'logical_cpus': os.cpu_count(), 'affinity_cpus': sorted(os.sched_getaffinity(0)),
            'requested_affinity_cpus': sorted(requested_cpus) if requested_cpus is not None else None,
            'python': platform.python_version()},
        'native_cli_comparison_scope': ('CPU affinity restricted; no exclusive CPU reservation. ' if requested_cpus is not None else 'Unrestricted current CPU affinity. ') + 'Internal paths; both processes remain live in every ABBA block; native TCP idle; one outstanding datagram per peer; no timed retries; operator-declared build profile; not competitor evidence.',
        'guest_udp_ipv6': args.guest_ipv6, 'local_udp_ipv6': args.local_ipv6,
        'control_api_tls': args.tls, 'node_mtls': args.mtls, 'latency_measurements': measurements,
        'scope': 'Owned shipped CLI/control-plane/Redis/daemon/KVM UDP; optional verified API TLS/node mTLS; Redis local plaintext.'}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
