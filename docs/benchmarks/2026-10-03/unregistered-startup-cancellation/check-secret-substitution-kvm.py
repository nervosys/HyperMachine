#!/usr/bin/env python3
"""Verify daemon-scoped substitution from an owned KVM guest to owned HTTPS."""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import json
import math
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


def cpu_snapshot(pid, stat_path=None):
    # Split after comm: Linux permits spaces and parentheses in that field.
    fields = (stat_path or Path(f'/proc/{pid}/stat')).read_text().rsplit(')', 1)[1].split()
    return {'start_ticks': int(fields[19]), 'user_ticks': int(fields[11]),
            'system_ticks': int(fields[12]), 'guest_ticks': int(fields[40])}


def cpu_delta(before, after):
    require(before['start_ticks'] == after['start_ticks'], 'daemon process identity changed')
    ticks = os.sysconf('SC_CLK_TCK')
    result = {key.removesuffix('_ticks') + '_seconds': (after[key] - before[key]) / ticks
              for key in ('user_ticks', 'system_ticks', 'guest_ticks')}
    require(all(value >= 0 for value in result.values()), 'CPU counter regressed')
    result['clock_ticks_per_second'] = ticks
    return result


def thread_snapshot(pid):
    result = {}
    for task in Path(f'/proc/{pid}/task').iterdir():
        try:
            counters = cpu_snapshot(pid, task / 'stat')
            name = (task / 'comm').read_text().strip()
        except FileNotFoundError:
            continue  # A short-lived thread can exit during collection.
        result[task.name] = {'name': name, 'counters': counters}
    return result


def thread_delta(before, after):
    rows = []
    for tid in before.keys() & after.keys():
        old, new = before[tid], after[tid]
        if old['counters']['start_ticks'] != new['counters']['start_ticks']:
            continue
        row = cpu_delta(old['counters'], new['counters'])
        row['name'] = new['name']
        rows.append(row)
    rows.sort(key=lambda row: row['user_seconds'] + row['system_seconds'], reverse=True)
    return {'surviving_threads': rows, 'threads_before': len(before), 'threads_after': len(after),
            'limits': ['Only threads present at both snapshots with matching start times are included',
                       'Thread names are Linux comm values truncated to 15 bytes',
                       'Thread totals need not equal process totals; snapshots are sequential']}


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('daemon', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--keepalive-rotation', action='store_true')
    parser.add_argument('--fresh-node-resume', action='store_true', help='Resume a shared paused guest after replacing the daemon process')
    parser.add_argument('--cpu-cores', type=int, choices=(1, 2), default=1)
    parser.add_argument('--bindings', type=int, choices=(1, 128), default=128)
    parser.add_argument('--timing-repetitions', type=int, choices=(0, 12), default=0)
    parser.add_argument('--request-concurrency', type=int, choices=(1, 8), default=1)
    args = parser.parse_args()
    require(os.name == 'posix' and Path('/dev/kvm').exists(), 'Linux KVM fixture required')
    require(not args.output.exists(), 'report already exists')
    hashes = {name: digest(getattr(args, name)) for name in ('daemon', 'kernel', 'initrd')}
    interfaces = json.loads(subprocess.check_output(['ip', '-j', '-4', 'addr', 'show', 'scope', 'global'], timeout=5))
    addresses = [a['local'] for interface in interfaces for a in interface['addr_info'] if 'local' in a]
    require(addresses, 'owned host IPv4 address required')
    address = str(ipaddress.IPv4Address(addresses[0]))
    checks = []
    http_timings = None
    received = []
    request_overlap = {'active': 0, 'peak': 0}
    overlap_lock = threading.Lock()
    keepalive_callbacks = {}
    keepalive_verified = False
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
            disable_nagle_algorithm = True
            def log_message(self, *arguments):
                pass
            def do_POST(self):
                length = int(self.headers.get('Content-Length', '0'))
                if length < 0 or length > 1024 * 1024:
                    self.send_error(413); return
                with overlap_lock:
                    request_overlap['active'] += 1
                    request_overlap['peak'] = max(request_overlap['peak'], request_overlap['active'])
                try:
                    received.append({'path': self.path, 'headers': dict(self.headers), 'body': self.rfile.read(length)})
                    callback = keepalive_callbacks.get(self.path)
                    if callback is not None:
                        callback()
                    self.send_response(200); self.send_header('Content-Length', '2'); self.end_headers(); self.wfile.write(b'OK')
                finally:
                    with overlap_lock: request_overlap['active'] -= 1
        class OwnedServer(ThreadingHTTPServer):
            request_queue_size = 32
        server = OwnedServer((address, 0), Handler)
        server.daemon_threads = True
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(root / 'server.pem', root / 'server.key')
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        token = 'hms_' + 'a' * 64
        special_secret = 'fixture &+%=/?:"\\'
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
                for value in (token, cluster_token, 'fixture-one', 'fixture-two', special_secret):
                    diagnostic = diagnostic.replace(value, '[redacted]')
                raise ValueError(f"owned guest command failed ({result['exit_code']}): {diagnostic}")
            return result['stdout']
        def request(sandbox, host='api.secret.example.test', expected=token, body_format='json'):
            before = len(received)
            if body_format == 'json':
                content_type = 'application/json'
                payload = json.dumps({'key': token})
            elif body_format == 'form':
                content_type = 'application/x-www-form-urlencoded'
                payload = 'key=' + token + '&keep=a+b&escaped=x%2By'
            elif body_format == 'raw':
                content_type = 'application/octet-stream'
                payload = 'prefix:' + token + ':suffix'
            elif body_format == 'binary':
                content_type = 'application/octet-stream'
                binary = b'\x00prefix:\xff' + token.encode() + b':suffix\x00\xfe'
                encoded = base64.b64encode(binary).decode()
                execute(sandbox, "printf '%s' " + shlex.quote(encoded) +
                        ' | /bin/busybox base64 -d > /tmp/hm-secret-binary-body')
                payload = '@/tmp/hm-secret-binary-body'
            else:
                raise ValueError('unknown owned fixture body format')
            command = shlex.join(['curl', '--silent', '--show-error', '--fail', '--max-time', '12',
                '--noproxy', '*', '--cacert', '/etc/ssl/certs/ca-certificates.crt',
                '--resolve', f'{host}:{server.server_port}:{address}', '--user', f'user:{token}',
                '--header', f'X-Fixture: {token}', '--header', 'Content-Type: ' + content_type,
                '--data-binary', payload,
                f'https://{host}:{server.server_port}/fixture?key={token}'])
            require(execute(sandbox, command) == 'OK', 'owned HTTPS response differs')
            require(len(received) == before + 1, 'owned upstream request count differs')
            row = received[-1]
            headers = {k.lower(): v for k, v in row['headers'].items()}
            require(headers.get('x-fixture') == expected, 'header substitution differs')
            require(headers.get('authorization') == 'Basic ' + base64.b64encode(f'user:{expected}'.encode()).decode(), 'Basic substitution differs')
            require(urllib.parse.parse_qs(urllib.parse.urlsplit(row['path']).query) == {'key': [expected]}, 'query substitution differs')
            if body_format == 'json':
                require(json.loads(row['body']) == {'key': expected}, 'JSON substitution differs')
            elif body_format == 'form':
                require(urllib.parse.parse_qs(row['body'].decode()) ==
                        {'key': [expected], 'keep': ['a b'], 'escaped': ['x+y']}, 'form substitution differs')
            elif body_format == 'binary':
                require(row['body'] == b'\x00prefix:\xff' + expected.encode() + b':suffix\x00\xfe',
                        'binary substitution changed surrounding bytes')
                execute(sandbox, 'rm /tmp/hm-secret-binary-body')
            else:
                require(row['body'] == ('prefix:' + expected + ':suffix').encode(), 'raw substitution differs')
            require(int(headers['content-length']) == len(row['body']), 'rewritten length differs')
        with (root / 'daemon.log').open('wb') as log:
            try:
                process = subprocess.Popen([str(args.daemon), '--network', '--port', str(api_port),
                    '--proxy-port', str(port()), '--snapshot-store', str(root / 'snapshots'),
                    '--tenant-reserved-cidr', address + '/32', '--egress-secrets-file', str(policy),
                    '--egress-upstream-ca', str(root / 'root.pem'), '--cpu-cores', str(args.cpu_cores)],
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
                require(int(execute(sandbox, '/bin/busybox grep -c ^processor /proc/cpuinfo').strip()) == args.cpu_cores,
                        'guest online CPU count differs')
                require('https' in execute(sandbox, 'curl --version'), 'guest client lacks HTTPS')
                ca = (root / 'root.pem').read_text()
                execute(sandbox, "printf '%s' " + shlex.quote(ca) + ' >> /etc/ssl/certs/ca-certificates.crt')
                request(sandbox); checks.append('Unscoped guest sends placeholders over verified owned HTTPS')
                def reload(value, hosts=None):
                    bindings = [{'placeholder': f'hms_{index:064x}', 'value': f'fixture-unused-{index}',
                                 'hosts': ['api.secret.example.test']} for index in range(args.bindings - 1)]
                    bindings.append({'placeholder': token, 'value': value, 'hosts': hosts or ['api.secret.example.test']})
                    document = {'version': 1, 'sandboxes': [] if value is None else [{'sandbox_id': sandbox,
                        'bindings': bindings}]}
                    count = (root / 'daemon.log').read_text().count('secret policy reload completed')
                    policy.write_text(json.dumps(document)); process.send_signal(signal.SIGHUP)
                    deadline = time.monotonic() + 10
                    while (root / 'daemon.log').read_text().count('secret policy reload completed') <= count:
                        require(process.poll() is None and time.monotonic() < deadline, 'policy reload timeout'); time.sleep(.02)
                reload('fixture-one'); request(sandbox, expected='fixture-one')
                checks.append('Exact sandbox scope rewrites headers, Basic auth, query and JSON with correct framing')
                request(sandbox, expected='fixture-one', body_format='raw')
                checks.append('Raw body substitution preserves surrounding bytes and fixes framing')
                request(sandbox, expected='fixture-one', body_format='binary')
                checks.append('Binary body substitution preserves NUL and non-UTF8 bytes with correct framing')
                request(sandbox, expected='fixture-one', body_format='form')
                checks.append('Form substitution preserves unrelated plus and percent decoding')
                refused = (root / 'daemon.log').read_text().count('secret policy reload refused; active scopes retained')
                policy.write_text('{invalid}'); process.send_signal(signal.SIGHUP)
                deadline = time.monotonic() + 10
                while (root / 'daemon.log').read_text().count('secret policy reload refused; active scopes retained') <= refused:
                    require(process.poll() is None and time.monotonic() < deadline, 'rejected policy reload timeout'); time.sleep(.02)
                request(sandbox, expected='fixture-one')
                checks.append('Malformed reload preserves the active secret observed upstream')
                reload(special_secret)
                for body_format in ('json', 'form', 'raw', 'binary'):
                    request(sandbox, expected=special_secret, body_format=body_format)
                checks.append('Delimiter-bearing secrets round-trip through query, Basic auth, JSON, form and raw bodies')
                request(sandbox, host='other.secret.example.test')
                checks.append('Different authenticated hostname preserves placeholders')
                wrong_host = 'wrong.secret.example.test'
                reload('fixture-two', hosts=['api.secret.example.test', wrong_host])
                before = len(received)
                rejected_command = shlex.join(['curl', '--silent', '--show-error', '--fail', '--max-time', '12',
                    '--noproxy', '*', '--cacert', '/etc/ssl/certs/ca-certificates.crt',
                    '--resolve', f'{wrong_host}:{server.server_port}:{address}',
                    '--header', f'X-Fixture: {token}', '--data-binary', token,
                    f'https://{wrong_host}:{server.server_port}/fixture'])
                rejected = api('POST', f'/sandboxes/{sandbox}/exec', {'cmd': rejected_command, 'timeout_secs': 20})
                require(rejected['exit_code'] != 0, 'upstream hostname mismatch accepted')
                require(len(received) == before, 'unverified upstream received HTTP request')
                decisions = api('GET', f'/sandboxes/{sandbox}/network/decisions')['decisions']
                require(any(row['kind'] == 'https-intercept' and row['name'] == wrong_host
                            for row in decisions), 'negative TLS request never reached interception')
                checks.append('Upstream hostname mismatch refuses the request before any owned HTTP payload arrives')
                reload('fixture-two'); request(sandbox, expected='fixture-two')
                checks.append('SIGHUP rotation preserves placeholders and uses updated secret')
                if args.keepalive_rotation:
                    reload('fixture-one')
                    keepalive_callbacks['/keepalive/rotate'] = lambda: reload('fixture-two')
                    keepalive_callbacks['/keepalive/revoke'] = lambda: reload(None)
                    common = ['curl', '--silent', '--show-error', '--fail', '--max-time', '12',
                        '--noproxy', '*', '--cacert', '/etc/ssl/certs/ca-certificates.crt',
                        '--resolve', f'api.secret.example.test:{server.server_port}:{address}',
                        '--header', f'X-Fixture: {token}', '--data-binary', token,
                        '--output', '/dev/null', '--write-out', '%{num_connects}\n']
                    command = []
                    for index, path in enumerate(('rotate', 'revoke', 'final')):
                        if index: command += ['--next']
                        command += common if index == 0 else common[1:]
                        command += [f'https://api.secret.example.test:{server.server_port}/keepalive/{path}']
                    before = len(received)
                    connections = execute(sandbox, shlex.join(command)).splitlines()
                    require(connections == ['1', '0', '0'], 'guest HTTPS connection was not reused')
                    require(len(received) == before + 3, 'keepalive request count differs')
                    for row, expected in zip(received[before:], ('fixture-one', 'fixture-two', token)):
                        headers = {key.lower(): value for key, value in row['headers'].items()}
                        require(row['body'] == expected.encode() and headers.get('x-fixture') == expected,
                                'keepalive rotation or revocation differs')
                        require(int(headers['content-length']) == len(row['body']), 'keepalive framing differs')
                    keepalive_callbacks.clear()
                    reload('fixture-two')
                    keepalive_verified = True
                    checks.append('One guest HTTPS connection observes rotation then revocation on subsequent requests')

                if args.timing_repetitions:
                    lines = 15000
                    execute(sandbox, shlex.join(['/bin/busybox', 'yes', token]) +
                            f' | /bin/busybox head -n {lines} > /tmp/hm-secret-large-body')
                    expected_body = (b'fixture-two\n') * lines
                    timed_command = shlex.join(['curl', '--silent', '--show-error', '--fail', '--max-time', '12',
                        '--noproxy', '*', '--cacert', '/etc/ssl/certs/ca-certificates.crt',
                        '--resolve', f'api.secret.example.test:{server.server_port}:{address}',
                        '--header', f'X-Fixture: {token}', '--header', 'Content-Type: application/octet-stream',
                        '--data-binary', '@/tmp/hm-secret-large-body', '--output', '/dev/null',
                        '--write-out', '%{time_total}\n', f'https://api.secret.example.test:{server.server_port}/large'])
                    samples = []
                    with overlap_lock:
                        require(request_overlap['active'] == 0, 'prior HTTP request remains active')
                        request_overlap['peak'] = 0
                    for iteration in range(3 + args.timing_repetitions):
                        if iteration == 3:
                            measured_threads = thread_snapshot(process.pid)
                            measured_cpu = cpu_snapshot(process.pid)
                            measured_driver_cpu = time.process_time()
                            measured_start = time.monotonic()
                        before = len(received)
                        if args.request_concurrency == 1:
                            output = execute(sandbox, timed_command)
                        else:
                            files = [f'/tmp/hm-secret-timing-{index}' for index in range(args.request_concurrency)]
                            script = "pids=''\nstatus=0\n"
                            for path in files:
                                script += f'({timed_command} > {path}) &\npids="$pids $!"\n'
                            script += 'for pid in $pids; do wait "$pid" || status=1; done\n'
                            script += 'if [ "$status" -ne 0 ]; then rm ' + ' '.join(files) + '; exit 1; fi\n'
                            script += 'cat ' + ' '.join(files) + '\nrm ' + ' '.join(files)
                            output = execute(sandbox, script)
                        batch = [float(value) for value in output.splitlines()]
                        require(len(batch) == args.request_concurrency and all(math.isfinite(value)
                                and value > 0 for value in batch), 'invalid curl timing batch')
                        require(len(received) == before + args.request_concurrency, 'large request count differs')
                        for row in received[before:]:
                            headers = {key.lower(): value for key, value in row['headers'].items()}
                            require(row['body'] == expected_body, 'large body substitution differs')
                            require(headers.get('x-fixture') == 'fixture-two', 'large header substitution differs')
                            require(int(headers['content-length']) == len(expected_body), 'large framing differs')
                        if iteration >= 3: samples.extend(batch)
                    measured_wall = time.monotonic() - measured_start
                    driver_cpu = time.process_time() - measured_driver_cpu
                    daemon_cpu = cpu_delta(measured_cpu, cpu_snapshot(process.pid))
                    daemon_threads = thread_delta(measured_threads, thread_snapshot(process.pid))
                    execute(sandbox, 'rm /tmp/hm-secret-large-body')
                    if args.request_concurrency > 1:
                        require(request_overlap['peak'] > 1, 'parallel HTTP overlap was not observed')
                    http_timings = {'warmup_requests': 3 * args.request_concurrency,
                                    'request_concurrency': args.request_concurrency,
                                    'peak_concurrent_http_requests': request_overlap['peak'],
                                    'input_bytes': (len(token) + 1) * lines,
                                    'output_bytes': len(expected_body), 'placeholders_per_body': lines,
                                    'samples_seconds': samples, 'metric': 'guest curl time_total',
                                    'server_tcp_nodelay': True,
                                    'batch_accounting': {
                                        'wall_seconds': measured_wall,
                                        'driver_and_https_server_cpu_seconds': driver_cpu,
                                        'daemon_cpu': daemon_cpu,
                                        'daemon_threads': daemon_threads,
                                        'scope': '12 measured batches including guest exec API and response validation',
                                        'limits': ['User CPU includes guest CPU; do not sum user and guest',
                                                   '100 Hz counters are quantized on this host',
                                                   'Driver and owned HTTPS server share one process',
                                                   'CPU accounting does not establish a causal bottleneck']}}

                    checks.append('Large raw requests preserve all 15000 replacements and framing during HTTPS timing')
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
                overlapping = 0
                with ThreadPoolExecutor(max_workers=1) as executor:
                    for _ in range(4):
                        api('POST', f'/sandboxes/{sandbox}/pause', {})
                        resumed = executor.submit(api, 'POST', f'/sandboxes/{sandbox}/resume', {})
                        if not resumed.done():
                            overlapping += 1
                        reload(None)
                        reload('fixture-two')
                        resumed.result(timeout=60)
                        request(sandbox, expected='fixture-two')
                require(overlapping > 0, 'resume/reload fixture did not overlap pending calls')
                checks.append('Four resume/revoke/re-add overlaps converge to the current exact sandbox scope')
                if args.fresh_node_resume:
                    api('POST', f'/sandboxes/{sandbox}/pause', {})
                    command = process.args
                    previous_pid = process.pid
                    process.terminate(); process.wait(timeout=10)
                    require(process.returncode == 0, 'previous daemon did not stop cleanly')
                    process = subprocess.Popen(command,
                        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd),
                                 HV2_CLUSTER_TOKEN=cluster_token, RUST_LOG='info'), stdout=log, stderr=log)
                    require(process.pid != previous_pid, 'fresh daemon process identity reused')
                    deadline = time.monotonic() + 30
                    while True:
                        require(process.poll() is None, 'fresh daemon exited during readiness')
                        try:
                            if any('base' in row.get('aliases', []) for row in api('GET', '/templates')): break
                        except OSError: pass
                        require(time.monotonic() < deadline, 'fresh daemon readiness timeout'); time.sleep(.05)
                    api('POST', f'/sandboxes/{sandbox}/resume', {})
                    request(sandbox, expected='fixture-two')
                    checks.append('Fresh daemon reconstructs paused network and exact secret scope from shared description')
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
    report = {'checks': checks, 'input_sha256': hashes, 'policy_bindings': args.bindings, 'guest_cpu_cores': args.cpu_cores, 'keepalive_rotation_verified': keepalive_verified,
              'fresh_node_resume_verified': args.fresh_node_resume, 'http_timings': http_timings,
              'upstream_requests': len(received),
              'processes_reaped': True, 'limits': [
              'No managed competitor measurement or performance claim', 'Timing requests open new TLS connections; optional keepalive check verifies reuse separately',
              'Pending resume/reload calls overlap; exact registration interleavings are not controlled',
              'Failed-pause reinsertion path not forced']}
    with args.output.open('x') as stream:
        json.dump(report, stream, indent=2); stream.write('\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
