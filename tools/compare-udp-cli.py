#!/usr/bin/env python3
"""Matched ABBA comparison of explicit CLI/configuration inputs through owned TLS/mTLS/KVM stacks."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import sys


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('checker', 'baseline', 'candidate', 'daemon', 'control-plane', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--peer-count', type=int, default=2)
    parser.add_argument('--payload-bytes', type=int, default=64)
    parser.add_argument('--samples-per-peer', type=int, default=1000)
    for engine in ('baseline', 'candidate'):
        parser.add_argument('--' + engine + '-guest-ipv6', action='store_true')
        parser.add_argument('--' + engine + '-local-ipv6', action='store_true')
    args = parser.parse_args()
    configurations = {engine: {'guest_ipv6': getattr(args, engine + '_guest_ipv6'),
                               'local_ipv6': getattr(args, engine + '_local_ipv6')}
                      for engine in ('baseline', 'candidate')}
    if not 2 <= args.peer_count <= 64 or not 5 <= args.payload_bytes <= 65507 or not 100 <= args.samples_per_peer <= 10000:
        parser.error('peers must be 2–64, payload 5–65507 bytes and samples 100–10000')
    inputs = {name: getattr(args, name).resolve(strict=True) for name in
              ('checker', 'baseline', 'candidate', 'daemon', 'control_plane', 'kernel', 'initrd')}
    hashes = {name: digest(path) for name, path in inputs.items()}
    if hashes['baseline'] == hashes['candidate'] and configurations['baseline'] == configurations['candidate']:
        parser.error('baseline and candidate binary contents must differ unless explicit address-family configurations differ')
    args.output.mkdir(parents=True, exist_ok=False)
    cohorts = []
    for index, engine in enumerate(('baseline', 'candidate', 'candidate', 'baseline')):
        if hashes != {name: digest(path) for name, path in inputs.items()}:
            raise RuntimeError('comparison inputs changed before cohort')
        name = f'{index}-{engine}'
        output = args.output / name
        command = [sys.executable, str(inputs['checker']), '--tls', '--mtls',
                   '--cli', str(inputs[engine]), '--output', str(output),
                   '--peer-count', str(args.peer_count), '--concurrent-payload-bytes', str(args.payload_bytes),
                   '--concurrent-samples', str(args.samples_per_peer)]
        for option in ('guest_ipv6', 'local_ipv6'):
            if configurations[engine][option]: command.append('--' + option.replace('_', '-'))
        for option in ('daemon', 'control_plane', 'kernel', 'initrd'):
            command.extend(('--' + option.replace('_', '-'), str(inputs[option])))
        with (args.output / (name + '-runner.log')).open('xb') as log:
            subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
        report = json.loads((output / 'report.json').read_text())
        if hashes != {key: digest(path) for key, path in inputs.items()}:
            raise RuntimeError('comparison inputs changed during cohort')
        if len(report['checks']) != 10 + 2 * int(configurations[engine]['guest_ipv6']) or report['guests_remaining'] != 0 or not all(report[key] for key in
                ('daemon_reaped', 'control_and_redis_reaped', 'cli_reaped', 'control_api_tls', 'node_mtls')):
            raise RuntimeError('correctness or cleanup gate failed')
        for option in ('daemon', 'control_plane', 'kernel', 'initrd', engine):
            if report['inputs_sha256'][str(inputs[option])] != hashes[option]:
                raise RuntimeError('checker input hash does not match comparison input')
        if report.get('guest_udp_ipv6', False) != configurations[engine]['guest_ipv6'] or report.get('local_udp_ipv6', False) != configurations[engine]['local_ipv6']:
            raise RuntimeError('measured address families differ from requested configuration')
        measurement, = report['latency_measurements']
        peers = measurement['peers']
        if measurement['concurrency'] != args.peer_count or len(peers) != args.peer_count or any(
                p['payload_bytes'] != args.payload_bytes or len(p['samples_ms']) != args.samples_per_peer for p in peers):
            raise RuntimeError('measurement dimensions differ from requested workload')
        cohorts.append({'engine': engine, 'configuration': configurations[engine], 'cohort': name,
                        'measured_rate': measurement['measured_roundtrips_per_second'],
                        'peers': [{key: p[key] for key in ('peer', 'p50_ms', 'p95_ms', 'p99_ms')} for p in peers]})
        print(json.dumps(cohorts[-1]), flush=True)
    means = {engine: statistics.mean(row['measured_rate'] for row in cohorts if row['engine'] == engine)
             for engine in ('baseline', 'candidate')}
    summary = {'inputs_sha256': hashes, 'input_paths': {name: str(path) for name, path in inputs.items()},
               'configurations': configurations,
               'comparison_kind': 'binary' if configurations['baseline'] == configurations['candidate'] else 'configuration' if hashes['baseline'] == hashes['candidate'] else 'binary_and_configuration',
               'peer_count': args.peer_count, 'payload_bytes': args.payload_bytes,
               'samples_per_peer': args.samples_per_peer, 'cohorts': cohorts, 'average_rates': means,
               'rate_change_percent': (means['candidate'] / means['baseline'] - 1) * 100}
    with (args.output / 'summary.json').open('x') as stream:
        json.dump(summary, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'average_rates': means, 'rate_change_percent': summary['rate_change_percent']}), flush=True)


if __name__ == '__main__':
    main()
