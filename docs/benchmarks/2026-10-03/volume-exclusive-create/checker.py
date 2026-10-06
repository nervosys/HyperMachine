#!/usr/bin/env python3
"""Check concurrent owned daemon volume creation without creating guests."""
import argparse
from concurrent.futures import ThreadPoolExecutor
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
    args = parser.parse_args()
    require(not args.output.exists(), 'report exists')
    hashes = {name: digest(getattr(args, name)) for name in ('daemon', 'kernel', 'initrd')}
    with tempfile.TemporaryDirectory(prefix='hm-volume-api-') as directory:
        root = Path(directory)
        selected_port = port()
        token = secrets.token_hex(32)
        def api(method, path, body=None):
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            request = urllib.request.Request(f'http://127.0.0.1:{selected_port}' + path, method=method,
                headers={'X-Hv2-Cluster-Token': token, 'Content-Type': 'application/json'},
                data=None if body is None else json.dumps(body).encode())
            try:
                response = opener.open(request, timeout=10)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                data = response.read()
                return response.status, json.loads(data) if data else None
        with (root / 'daemon.log').open('wb') as log:
            process = subprocess.Popen([str(args.daemon), '--no-template', '--port', str(selected_port),
                '--proxy-port', str(port()), '--snapshot-store', str(root / 'snapshots'),
                '--volume-dir', str(root / 'volumes')],
                env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd),
                         HV2_CLUSTER_TOKEN=token, RUST_LOG='warn'), stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 20
                while True:
                    require(process.poll() is None, 'owned daemon exited')
                    try:
                        if api('GET', '/sandboxes') == (200, []): break
                    except OSError:
                        pass
                    require(time.monotonic() < deadline, 'owned readiness timeout'); time.sleep(.05)
                barrier = threading.Barrier(16)
                def create(_):
                    barrier.wait(timeout=15)
                    return api('POST', '/volumes', {'name': 'owned-concurrent'})
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
                status, listed = api('GET', '/volumes')
                require(status == 200 and len(listed) == 1 and listed[0]['volumeID'] == winner['volumeID'], 'volume listing differs')
                require(api('DELETE', '/volumes/' + winner['volumeID'])[0] == 204, 'owned volume cleanup failed')
                require(api('GET', '/volumes') == (200, []), 'owned volume remains')
            finally:
                process.terminate()
                try: process.wait(timeout=10)
                except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
    require(hashes == {name: digest(getattr(args, name)) for name in hashes}, 'input changed')
    report = {'input_sha256': hashes, 'concurrent_requests': 16, 'created': 1, 'conflicts': conflicts,
              'checks': ['One concurrent creator wins', 'Winner token matches published metadata',
                         'Duplicate preserves metadata and data', 'One volume listed and cleaned up'],
              'processes_reaped': True, 'guests_created': 0,
              'limits': ['Single local daemon; multi-node shared filesystem not exercised',
                         'No power-loss durability, performance or competitor comparison']}
    with args.output.open('x') as stream:
        json.dump(report, stream, indent=2); stream.write('\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
