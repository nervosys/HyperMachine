#!/usr/bin/env python3
"""Verify owned daemon startup and policy reload without creating guests."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import tempfile
import time
import urllib.request


def require(condition, message):
    if not condition:
        raise ValueError(message)


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def wait_for(check, process, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        require(process.poll() is None, 'owned daemon exited before verification')
        try:
            if check():
                return
        except (OSError, ValueError):
            pass
        time.sleep(.05)
    raise ValueError('owned daemon verification deadline exceeded')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('daemon', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    require(os.name == 'posix', 'Linux fixture required')
    hashes = {name: hashlib.sha256(getattr(args, name).read_bytes()).hexdigest()
              for name in ('daemon', 'kernel', 'initrd')}
    checks = []
    processes = []
    with tempfile.TemporaryDirectory(prefix='hm-secret-daemon-') as directory:
        root = Path(directory)
        policy = root / 'policy.json'
        empty = {'version': 1, 'sandboxes': []}
        policy.write_text(json.dumps(empty)); policy.chmod(0o600)
        env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd),
                   HV2_CLUSTER_TOKEN=secrets.token_hex(32), RUST_LOG='info')
        def command(selected, network=True):
            return [str(args.daemon), '--no-template', '--port', str(port()), '--proxy-port', str(port()),
                    '--snapshot-store', str(root / 'snapshots'), '--egress-secrets-file', str(selected)] + (['--network'] if network else [])
        def refusal(selected, expected, network=True):
            result = subprocess.run(command(selected, network), env=env, capture_output=True, timeout=20)
            require(result.returncode != 0 and expected.encode() in result.stderr, 'startup refusal differs')
        refusal(policy, 'requires --network', network=False)
        checks.append('Secret policy requires networking')
        policy.chmod(0o644); refusal(policy, 'private secret policy validation failed'); policy.chmod(0o600)
        checks.append('Unsafe policy permissions refuse startup')
        link = root / 'link.json'; link.symlink_to(policy)
        refusal(link, 'private secret policy validation failed')
        checks.append('Symlink policy refuses startup')
        policy.write_text('{invalid}')
        refusal(policy, 'private secret policy validation failed')
        checks.append('Malformed policy refuses startup')
        policy.write_text(json.dumps(empty))
        argv = command(policy)
        endpoint = 'http://127.0.0.1:' + argv[argv.index('--port') + 1] + '/sandboxes'
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def inventory():
            request = urllib.request.Request(endpoint, headers={'X-Hv2-Cluster-Token': env['HV2_CLUSTER_TOKEN']})
            with opener.open(request, timeout=1) as response:
                return json.load(response)
        log = root / 'daemon.log'
        with log.open('wb') as stream:
            process = subprocess.Popen(argv, env=env, stdout=stream, stderr=stream)
            processes.append(process)
            try:
                wait_for(lambda: inventory() == [], process)
                checks.append('Private valid policy starts an empty owned daemon')
                policy.write_text('{invalid}')
                process.send_signal(signal.SIGHUP)
                wait_for(lambda: 'secret policy reload refused; active scopes retained' in log.read_text(), process)
                require(inventory() == [], 'rejected reload changed owned inventory')
                checks.append('Malformed SIGHUP reload is refused and daemon remains responsive')
                policy.write_text(json.dumps(empty))
                process.send_signal(signal.SIGHUP)
                wait_for(lambda: 'secret policy reload completed' in log.read_text(), process)
                require(inventory() == [], 'valid reload changed owned inventory')
                checks.append('Valid SIGHUP reload completes and daemon remains responsive')
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill(); process.wait(timeout=5)
    require(all(process.poll() is not None for process in processes), 'owned process remains')
    require(hashes == {name: hashlib.sha256(getattr(args, name).read_bytes()).hexdigest()
                       for name in hashes}, 'input artifact changed')
    report = {'checks': checks, 'input_sha256': hashes, 'processes_reaped': True,
              'guests_created': 0, 'limits': ['No guest substitution or policy retention contents observed',
                                             'No performance comparison or managed competitor measurement']}
    with args.output.open('x') as stream:
        json.dump(report, stream, indent=2); stream.write('\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
