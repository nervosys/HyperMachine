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
    args = parser.parse_args()
    require(not args.output.exists(), 'report exists')
    hashes = {name: digest(getattr(args, name)) for name in ('daemon', 'kernel', 'initrd')}
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
                winning_node = next(index for index, (status, _) in enumerate(results) if status == 201) % args.nodes
                stop(active[winning_node]); start(winning_node)
                require(api('GET', '/volumes/' + winner['volumeID'], node=winning_node) == (200, winner), 'restart changed token')
                require((volume / 'meta.json').read_bytes() == metadata, 'node/restart changed metadata')
                require((volume / 'data' / 'marker').read_bytes() == b'owned persistent marker', 'restart changed data')
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
                         'Winning daemon restart preserves token and data', 'One volume listed and cleaned up on all nodes'],
              'processes_reaped': True, 'guests_created': 0,
              'limits': ['Independent daemons share one local filesystem; network filesystem not exercised',
                         'No power-loss durability, performance or competitor comparison']}
    with args.output.open('x') as stream:
        json.dump(report, stream, indent=2); stream.write('\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
