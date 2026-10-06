#!/usr/bin/env python3
"""Compare matched owned release daemons through verified KVM guest HTTPS."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import statistics
import subprocess
import sys


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('baseline', 'candidate', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--request-concurrency', type=int, choices=(1, 8), default=1)
    parser.add_argument('--cpu-cores', type=int, choices=(1, 2), default=1)
    args = parser.parse_args()
    require(not args.output.exists(), 'output directory exists')
    checker = Path(__file__).with_name('check-secret-substitution-kvm.py')
    inputs = {name: digest(getattr(args, name)) for name in ('baseline', 'candidate', 'kernel', 'initrd')}
    require(inputs['baseline'] != inputs['candidate'], 'comparison binaries are identical')
    checker_hash = digest(checker)
    args.output.mkdir()
    runs = []
    for pair in range(2):
        for engine in (('baseline', 'candidate') if pair == 0 else ('candidate', 'baseline')):
            stem = f'pair-{pair}-{engine}'
            report_path = args.output / (stem + '.json')
            with (args.output / (stem + '.log')).open('xb') as log:
                result = subprocess.run([sys.executable, str(checker), '--bindings', '128',
                    '--timing-repetitions', '12', '--daemon', str(getattr(args, engine)),
                    '--request-concurrency', str(args.request_concurrency),
                    '--cpu-cores', str(args.cpu_cores),
                    '--kernel', str(args.kernel), '--initrd', str(args.initrd), '--output', str(report_path)],
                    stdout=log, stderr=log)
            require(result.returncode == 0, 'owned KVM fixture failed: ' + stem)
            report = json.loads(report_path.read_text())
            require(report['input_sha256'] == {'daemon': inputs[engine], 'kernel': inputs['kernel'],
                    'initrd': inputs['initrd']}, 'input identity differs')
            require(report['policy_bindings'] == 128 and report['guest_cpu_cores'] == args.cpu_cores
                    and len(report['checks']) == 15
                    and report['upstream_requests'] == 19 + 15 * args.request_concurrency
                    and report['processes_reaped'] is True,
                    'owned correctness/lifecycle contract differs')
            timings = report['http_timings']
            require(timings['input_bytes'] == 1035000 and timings['output_bytes'] == 180000
                    and timings['placeholders_per_body'] == 15000
                    and timings['warmup_requests'] == 3 * args.request_concurrency
                    and timings['request_concurrency'] == args.request_concurrency
                    and timings['server_tcp_nodelay'] is True, 'timing fixture differs')
            if args.request_concurrency > 1:
                require(timings['peak_concurrent_http_requests'] > 1, 'HTTP concurrency unverified')
            samples = timings['samples_seconds']
            require(len(samples) == 12 * args.request_concurrency and all(isinstance(value, (int, float))
                and not isinstance(value, bool) and math.isfinite(value) and value > 0
                for value in samples), 'invalid request timings')
            median = statistics.median(samples) * 1000
            runs.append({'pair': pair, 'engine': engine, 'report': report_path.name,
                         'median_ms': median, 'samples_seconds': samples})
            print(f'{stem}: {median:.3f} ms; correctness and cleanup passed', flush=True)
    baseline = [run['median_ms'] for run in runs if run['engine'] == 'baseline']
    candidate = [run['median_ms'] for run in runs if run['engine'] == 'candidate']
    before, after = statistics.median(baseline), statistics.median(candidate)
    require(inputs == {name: digest(getattr(args, name)) for name in inputs}, 'binary/input changed')
    require(digest(checker) == checker_hash, 'checker changed')
    summary = {'input_sha256': inputs, 'checker_sha256': checker_hash, 'pairs': 2, 'runs': runs,
               'request_concurrency': args.request_concurrency,
               'guest_cpu_cores': args.cpu_cores,
               'baseline_run_medians_ms': baseline, 'candidate_run_medians_ms': candidate,
               'baseline_ms': before, 'candidate_ms': after, 'change_percent': (after / before - 1) * 100,
               'limits': ['Two alternating local WSL pairs; unpinned processes',
                         'Synthetic large raw body, 128 bindings and new TLS connection per request',
                         'Includes owned server and guest networking; no service P99 or fleet throughput',
                         'Release build identities require external source/build evidence',
                         'No managed competitor measurement']}
    with (args.output / 'summary.json').open('x') as stream:
        json.dump(summary, stream, indent=2); stream.write('\n')
    print(f'Observed median of run medians: {before:.3f} -> {after:.3f} ms ({summary["change_percent"]:+.1f}%)')


if __name__ == '__main__':
    main()
