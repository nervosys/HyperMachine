#!/usr/bin/env python3
"""Compare one and two guest vCPUs with one immutable owned daemon binary."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import statistics
import subprocess
import sys


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('daemon', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    checker = Path(__file__).with_name('check-secret-substitution-kvm.py')
    inputs = {name: digest(getattr(args, name)) for name in ('daemon', 'kernel', 'initrd')}
    checker_hash = digest(checker)
    args.output.mkdir()  # Refuse overwriting an existing evidence directory.
    runs = []
    for index, cores in enumerate((1, 2, 2, 1)):
        stem = f'run-{index}-{cores}-cpu'
        report_path = args.output / (stem + '.json')
        with (args.output / (stem + '.log')).open('xb') as log:
            result = subprocess.run([sys.executable, str(checker), '--daemon', str(args.daemon),
                '--kernel', str(args.kernel), '--initrd', str(args.initrd), '--output', str(report_path),
                '--cpu-cores', str(cores), '--request-concurrency', '8', '--timing-repetitions', '12'],
                stdout=log, stderr=log)
        if result.returncode:
            raise ValueError('owned fixture failed: ' + stem)
        report = json.loads(report_path.read_text())
        timing = report['http_timings']
        if not (report['input_sha256'] == inputs and report['guest_cpu_cores'] == cores
                and report['processes_reaped'] is True and len(report['checks']) == 15
                and report['upstream_requests'] == 139 and report['policy_bindings'] == 128
                and timing['request_concurrency'] == 8 and timing['peak_concurrent_http_requests'] > 1
                and timing['warmup_requests'] == 24 and timing['server_tcp_nodelay'] is True
                and timing['input_bytes'] == 1035000 and timing['output_bytes'] == 180000
                and timing['placeholders_per_body'] == 15000
                and len(timing['samples_seconds']) == 96
                and all(isinstance(value, (int, float)) and not isinstance(value, bool)
                        and math.isfinite(value) and value > 0 for value in timing['samples_seconds'])):
            raise ValueError('fixture contract differs: ' + stem)
        runs.append({'cpu_cores': cores, 'report': report_path.name,
                     'median_request_ms': statistics.median(timing['samples_seconds']) * 1000,
                     'batch_accounting': timing['batch_accounting']})
        print(stem + ': correctness and cleanup passed', flush=True)
    if inputs != {name: digest(getattr(args, name)) for name in inputs} or digest(checker) != checker_hash:
        raise ValueError('input or checker changed')
    summary = {'input_sha256': inputs, 'checker_sha256': checker_hash, 'runs': runs,
               'limits': ['Eight clients inside one guest; no managed competitor measurement',
                          'Two cohorts per configuration; CPU placement is not fixed',
                          'Extra guest vCPU changes resource allocation; this is not a code speedup',
                          'Batch wall includes guest exec API and response validation']}
    (args.output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')


if __name__ == '__main__':
    main()
