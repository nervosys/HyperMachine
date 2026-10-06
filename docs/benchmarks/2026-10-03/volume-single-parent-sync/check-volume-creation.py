#!/usr/bin/env python3
"""Check concurrent owned daemon volume creation without creating guests."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
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
    parser.add_argument('--nodes', type=int, choices=(1, 2), default=2)
    parser.add_argument('--atomic-uploads', action='store_true')
    parser.add_argument('--upload-timing-repetitions', type=int, choices=(0, 12), default=0)
    args = parser.parse_args()
    require(not args.output.exists(), 'report exists')
    hashes = {name: digest(getattr(args, name)) for name in ('daemon', 'kernel', 'initrd')}
    upload_timings = None
    with tempfile.TemporaryDirectory(prefix='hm-volume-api-') as directory:
        root = Path(directory)
        selected_ports = [port() for _ in range(args.nodes)]
        token = secrets.token_hex(32)
        def api(method, path, body=None, node=0):
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            request = urllib.request.Request(f'http://127.0.0.1:{selected_ports[node]}' + path, method=method,
                headers={'X-Hv2-Cluster-Token': token, 'Content-Type': 'application/json'},
                data=None if body is None else json.dumps(body).encode())
            try:
                response = opener.open(request, timeout=10)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                data = response.read()
                return response.status, json.loads(data) if data else None
        def content(method, path, volume_token, data=None, node=0):
            request = urllib.request.Request(f'http://127.0.0.1:{selected_ports[node]}' + path,
                method=method, headers={'Authorization': 'Bearer ' + volume_token,
                                       'Content-Type': 'application/octet-stream'}, data=data)
            try:
                response = urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=10)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                return response.status, response.read()
        processes = []
        active = [None] * args.nodes
        def stop(process):
            process.terminate()
            try: process.wait(timeout=10)
            except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
        with ExitStack() as stack:
            def start(node):
                log = stack.enter_context((root / f'daemon-{node}.log').open('ab'))
                process = subprocess.Popen([str(args.daemon), '--no-template', '--port', str(selected_ports[node]),
                    '--proxy-port', str(port()), '--snapshot-store', str(root / f'snapshots-{node}'),
                    '--volume-dir', str(root / 'volumes')],
                    env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd),
                             HV2_CLUSTER_TOKEN=token, RUST_LOG='warn'), stdout=log, stderr=log)
                processes.append(process); active[node] = process
                deadline = time.monotonic() + 20
                while True:
                    require(process.poll() is None, 'owned daemon exited')
                    try:
                        if api('GET', '/sandboxes', node=node) == (200, []): return
                    except OSError:
                        pass
                    require(time.monotonic() < deadline, 'owned readiness timeout'); time.sleep(.05)
            try:
                for node in range(args.nodes): start(node)
                barrier = threading.Barrier(16)
                def create(index):
                    barrier.wait(timeout=15)
                    return api('POST', '/volumes', {'name': 'owned-concurrent'}, node=index % args.nodes)
                with ThreadPoolExecutor(max_workers=16) as executor:
                    results = list(executor.map(create, range(16)))
                winners = [body for status, body in results if status == 201]
                conflicts = sum(status == 409 for status, _ in results)
                require(len(winners) == 1 and conflicts == 15, 'concurrent create statuses differ')
                winner = winners[0]
                volume = root / 'volumes' / winner['volumeID']
                metadata = (volume / 'meta.json').read_bytes()
                require(json.loads(metadata)['token'] == winner['token'], 'winner token differs from disk')
                (volume / 'data' / 'marker').write_bytes(b'owned persistent marker')
                require(api('POST', '/volumes', {'name': 'owned-concurrent'})[0] == 409, 'duplicate did not conflict')
                require((volume / 'meta.json').read_bytes() == metadata, 'duplicate changed metadata')
                require((volume / 'data' / 'marker').read_bytes() == b'owned persistent marker', 'duplicate changed data')
                for node in range(args.nodes):
                    status, listed = api('GET', '/volumes', node=node)
                    require(status == 200 and len(listed) == 1 and listed[0]['volumeID'] == winner['volumeID'], 'volume listing differs')
                    require(api('GET', '/volumes/' + winner['volumeID'], node=node) == (200, winner), 'node published token differs')
                    require(api('POST', '/volumes', {'name': 'owned-concurrent'}, node=node)[0] == 409, 'node duplicate did not conflict')
                content_path = '/volumecontent/' + winner['volumeID'] + '/file?path=/nested/deep/upload.bin&force=true&mode=384'
                if args.atomic_uploads: content_path += '&atomic=true'
                payload = bytes(range(256)) * 4096 + b'owned-tail'
                status, uploaded = content('PUT', content_path, winner['token'], payload)
                require(status == 201 and json.loads(uploaded)['size'] == len(payload), 'binary upload failed')
                for node in range(args.nodes):
                    require(content('GET', content_path, winner['token'], node=node) == (200, payload), 'cross-node upload differs')
                if args.upload_timing_repetitions:
                    samples = []
                    for iteration in range(3 + args.upload_timing_repetitions):
                        started = time.monotonic()
                        status, uploaded = content('PUT', content_path, winner['token'], payload)
                        elapsed = time.monotonic() - started
                        require(status == 201 and json.loads(uploaded)['size'] == len(payload), 'timed upload failed')
                        for node in range(args.nodes):
                            require(content('GET', content_path, winner['token'], node=node) == (200, payload), 'timed upload bytes differ')
                        if iteration >= 3: samples.append(elapsed)
                    upload_timings = {'samples_seconds': samples, 'warmup_uploads': 3,
                                      'input_bytes': len(payload), 'atomic': args.atomic_uploads,
                                      'metric': 'host HTTP PUT through response including flush; readback excluded'}
                replacement = b'owned-replacement\x00\xff'
                require(content('PUT', content_path, winner['token'], replacement)[0] == 201, 'replacement upload failed')
                require(content('PUT', content_path, 'owned-wrong-token', b'refused')[0] == 401, 'wrong token accepted')
                if args.atomic_uploads:
                    root_content_path = '/volumecontent/' + winner['volumeID'] + '/file?path=/root.bin&atomic=true'
                    require(content('PUT', root_content_path, winner['token'], replacement)[0] == 201, 'root atomic upload failed')
                    for node in range(args.nodes):
                        require(content('GET', root_content_path, winner['token'], node=node) == (200, replacement), 'root atomic bytes differ')
                    # Owned incomplete upload: end the write stream before its declared length.
                    with socket.create_connection(('127.0.0.1', selected_ports[0]), timeout=10) as connection:
                        headers = (f'PUT {content_path} HTTP/1.1\r\nHost: localhost\r\n'
                                   f'Authorization: Bearer {winner["token"]}\r\n'
                                   'Content-Length: 100\r\nConnection: close\r\n\r\n')
                        connection.sendall(headers.encode() + b'partial')
                        connection.shutdown(socket.SHUT_WR)
                        response = b''
                        while True:
                            chunk = connection.recv(4096)
                            if not chunk: break
                            response += chunk
                            require(len(response) < 65536, 'unexpected incomplete-upload response size')
                        require(response.startswith(b'HTTP/1.1 400 '), 'incomplete upload was not refused')
                    require(content('GET', content_path, winner['token']) == (200, replacement),
                            'incomplete atomic upload changed previous bytes')
                    require(not list((volume / 'data').rglob('.hm-upload-*')), 'atomic staging file remains')
                winning_node = next(index for index, (status, _) in enumerate(results) if status == 201) % args.nodes
                stop(active[winning_node]); start(winning_node)
                require(api('GET', '/volumes/' + winner['volumeID'], node=winning_node) == (200, winner), 'restart changed token')
                require((volume / 'meta.json').read_bytes() == metadata, 'node/restart changed metadata')
                require((volume / 'data' / 'marker').read_bytes() == b'owned persistent marker', 'restart changed data')
                for node in range(args.nodes):
                    require(content('GET', content_path, winner['token'], node=node) == (200, replacement), 'restart changed uploaded data')
                require(api('DELETE', '/volumes/' + winner['volumeID'])[0] == 204, 'owned volume cleanup failed')
                for node in range(args.nodes):
                    require(api('GET', '/volumes', node=node) == (200, []), 'owned volume remains')
            finally:
                for process in processes:
                    if process.poll() is None: stop(process)
                require(all(process.poll() is not None for process in processes), 'owned process remains')
    require(hashes == {name: digest(getattr(args, name)) for name in hashes}, 'input changed')
    report = {'input_sha256': hashes, 'nodes': args.nodes, 'concurrent_requests': 16, 'created': 1, 'conflicts': conflicts,
              'checks': ['One concurrent creator wins', 'Winner token matches published metadata',
                         'Duplicate preserves metadata and data', 'All nodes observe the same winning token',
                         'Winning daemon restart preserves token and data', 'One volume listed and cleaned up on all nodes',
                         'Nested binary upload and replacement visible on all nodes after restart',
                         'Wrong volume bearer token refuses overwrite'],
              'processes_reaped': True, 'guests_created': 0, 'atomic_uploads': args.atomic_uploads, 'upload_timings': upload_timings,
              'limits': ['Independent daemons share one local filesystem; network filesystem not exercised',
                         'No power-loss durability, performance or competitor comparison']}
    if args.atomic_uploads:
        report['checks'].append('Root-level atomic upload succeeds with the pinned data-root flush')
        report['checks'].append('Incomplete atomic upload preserves prior file and removes staging file')
    with args.output.open('x') as stream:
        json.dump(report, stream, indent=2); stream.write('\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
