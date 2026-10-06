#!/usr/bin/env python3
"""Compare verified atomic and in-place uploads with one owned daemon binary."""
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
    checker = Path(__file__).with_name('check-volume-creation.py')
    inputs = {name: digest(getattr(args, name)) for name in ('daemon', 'kernel', 'initrd')}
    checker_hash = digest(checker)
    args.output.mkdir()
    runs = []
    for index, atomic in enumerate((False, True, True, False)):
        stem = f'run-{index}-' + ('atomic' if atomic else 'in-place')
        report_path = args.output / (stem + '.json')
        command = [sys.executable, str(checker), '--daemon', str(args.daemon),
            '--kernel', str(args.kernel), '--initrd', str(args.initrd), '--output', str(report_path),
            '--nodes', '2', '--upload-timing-repetitions', '12']
        if atomic:
            command.append('--atomic-uploads')
        with (args.output / (stem + '.log')).open('xb') as log:
            result = subprocess.run(command, stdout=log, stderr=log)
        if result.returncode:
            raise ValueError('owned fixture failed: ' + stem)
        report = json.loads(report_path.read_text())
        timing = report['upload_timings']
        samples = timing['samples_seconds']
        if not (report['input_sha256'] == inputs and report['processes_reaped'] is True
                and report['created'] == 1 and report['conflicts'] == 15
                and report['nodes'] == 2 and report['guests_created'] == 0
                and report['atomic_uploads'] is atomic and len(report['checks']) == (10 if atomic else 8)
                and timing['warmup_uploads'] == 3 and timing['input_bytes'] == 1048586
                and timing['atomic'] is atomic and len(samples) == 12
                and all(isinstance(v, (int, float)) and not isinstance(v, bool)
                        and math.isfinite(v) and v > 0 for v in samples)):
            raise ValueError('upload fixture contract differs: ' + stem)
        runs.append({'atomic': atomic, 'report': report_path.name,
                     'median_ms': statistics.median(samples) * 1000})
        print(stem + ': correctness and cleanup passed', flush=True)
    if inputs != {name: digest(getattr(args, name)) for name in inputs} or digest(checker) != checker_hash:
        raise ValueError('input or checker changed')
    summary = {'input_sha256': inputs, 'checker_sha256': checker_hash, 'runs': runs,
               'limits': ['One immutable daemon binary, two alternating pairs on local WSL storage',
                          'Sequential host HTTP PUTs; exact readback excluded from request timing',
                          'No guest, power-loss, network-filesystem, fleet P99 or competitor measurement',
                          'Build profile and provenance require external evidence']}
    (args.output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')


if __name__ == '__main__':
    main()
