#!/usr/bin/env python3
"""Alternate owned baseline/candidate synthetic secret replacement binaries."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import statistics
import subprocess


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('baseline', 'candidate', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    require(not args.output.exists(), 'report already exists')
    hashes = {name: digest(getattr(args, name)) for name in ('baseline', 'candidate')}
    runs = []
    contract = None
    for pair in range(4):
        for engine in (('baseline', 'candidate') if pair % 2 == 0 else ('candidate', 'baseline')):
            result = subprocess.run([str(getattr(args, engine))], capture_output=True, timeout=30, check=True)
            report = json.loads(result.stdout)
            require(report['version'] == 1 and report['samples_per_case'] == 9
                    and report['iterations_per_sample'] == 10, 'benchmark metadata differs')
            require(len(report['cases']) == 8, 'benchmark case count differs')
            current = []
            for case in report['cases']:
                samples = case['samples_us']
                require(len(samples) == 9 and all(isinstance(value, (int, float))
                    and not isinstance(value, bool) and math.isfinite(value) and value > 0
                    for value in samples), 'invalid timing samples')
                current.append({key: case[key] for key in
                    ('case', 'input_bytes', 'bindings', 'output_bytes', 'output_checksum')})
            require(len({case['case'] for case in current}) == 8, 'duplicate benchmark case')
            if contract is None: contract = current
            require(current == contract, 'input/output comparison contract differs')
            runs.append({'pair': pair, 'engine': engine, 'report': report})
    summaries = []
    for index, case in enumerate(contract):
        baseline = [statistics.median(run['report']['cases'][index]['samples_us'])
                    for run in runs if run['engine'] == 'baseline']
        candidate = [statistics.median(run['report']['cases'][index]['samples_us'])
                     for run in runs if run['engine'] == 'candidate']
        before, after = statistics.median(baseline), statistics.median(candidate)
        summaries.append({'case': case['case'], 'baseline_run_medians_us': baseline,
            'candidate_run_medians_us': candidate, 'baseline_us': before, 'candidate_us': after,
            'change_percent': (after / before - 1) * 100})
    require(hashes == {name: digest(getattr(args, name)) for name in hashes}, 'binary changed')
    output = {'binary_sha256': hashes, 'pairs': 4, 'runs': runs, 'summary': summaries,
              'limits': ['Synthetic raw replacement only; no HTTP/TLS/KVM performance',
                         'Unpinned single processes; batch averages are not service P99',
                         'Output checksums are non-cryptographic; correctness needs separate tests',
                         'No managed competitor comparison']}
    with args.output.open('x') as stream:
        json.dump(output, stream, indent=2); stream.write('\n')
    for case in summaries:
        print(f"{case['case']}: {case['baseline_us']:.2f} -> {case['candidate_us']:.2f} us ({case['change_percent']:+.1f}%)")


if __name__ == '__main__':
    main()
