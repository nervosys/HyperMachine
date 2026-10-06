#!/usr/bin/env python3
"""Counterbalance accepted/layer-compare daemons with matched Firecracker controls."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('baseline', 'candidate', 'firecracker', 'kernel', 'initrd', 'output', 'candidate-context', 'lifecycle-report'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--pairs', type=int, default=2)
    parser.add_argument('--concurrency', type=int, default=8)
    args = parser.parse_args()
    if not 1 <= args.pairs <= 8 or not 1 <= args.concurrency <= 100 or args.output.exists():
        raise ValueError('invalid profile or existing evidence')
    spec = importlib.util.spec_from_file_location('borrowed_prepared', Path(__file__).with_name('bench-prepared-engines.py'))
    warm = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(warm)
    paths = {name: getattr(args, name).resolve(strict=True) for name in ('baseline', 'candidate', 'firecracker', 'kernel', 'initrd')}
    for name, path in paths.items():
        setattr(args, name, path)
    paths.update(driver=Path(__file__).resolve(), coordinator=Path(warm.__file__).resolve(),
                 engines=Path(warm.engines.__file__).resolve(), firecracker_harness=Path(warm.fc.__file__).resolve(),
                 candidate_context=args.candidate_context.resolve(strict=True))
    hashes = {name: warm.engines.digest(path) for name, path in paths.items()}
    lifecycle = json.loads(args.lifecycle_report.read_text())
    if (lifecycle.get('all_variants_passed') is not True or lifecycle.get('cleanup_verified') is not True
            or lifecycle.get('artifacts_unchanged') is not True
            or any(lifecycle['artifact_sha256'].get(name) != hashes[name] for name in ('baseline','candidate','kernel','initrd'))):
        raise ValueError('named-source deletion/pause/resume lifecycle gate failed')
    hashes['lifecycle_report'] = warm.engines.digest(args.lifecycle_report)
    paths['lifecycle_report'] = args.lifecycle_report.resolve(strict=True)
    context = json.loads(args.candidate_context.read_text())
    if (context['candidate_sha256'] != hashes['candidate'] or context['baseline_sha256'] != hashes['baseline']
            or context['build_exit_code'] != 0 or context['production_runtime_changed'] is not False
            or context['intervention'] != 'layered_restore_compare_before_write' or hashes['candidate'] == hashes['baseline']):
        raise ValueError('candidate build binding differs')
    affinity = sorted(os.sched_getaffinity(0))[:8]
    if len(affinity) != 8:
        raise ValueError('requires eight available CPUs')
    os.sched_setaffinity(0, affinity)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {'pairs': args.pairs, 'concurrency': args.concurrency, 'cpu_affinity': affinity,
              'artifact_sha256': hashes, 'runs': [], 'success': False, 'runtime_change_adopted': False,
              'managed_competitor_win_established': False, 'intervention': context['intervention'],
              'candidate_source_sha256': context['candidate_source_sha256'],
              'order': 'fresh-daemon baseline/candidate AB/BA with internal HM/FC AB/BA',
              'limitations': ['Shared WSL nested KVM host; uncontrolled background load',
                              'Fresh prepared sources per variant; preparation excluded from restore timing',
                              'Two internal pairs per variant; batches are not independent hosts',
                              'No allocator probe, preload, tuning, or diagnostic logging',
                              'Compare-before-write adds a page read and comparison per present layered page']}
    original_argv = sys.argv
    try:
        for pair in range(args.pairs):
            for variant in (('baseline', 'candidate') if pair % 2 == 0 else ('candidate', 'baseline')):
                path = args.output.parent / f'{pair}-{variant}.json'
                row = {'pair': pair, 'variant': variant, 'success': False}
                sys.argv = ['bench-prepared-engines.py', '--pairs', '2', '--concurrency', str(args.concurrency),
                            '--hypermachine', str(getattr(args, variant)), '--output', str(path)]
                for name in ('firecracker', 'kernel', 'initrd'):
                    sys.argv += ['--' + name, str(getattr(args, name))]
                try:
                    code = warm.main()
                    if path.exists():
                        row['prepared_report'] = json.loads(path.read_text())
                    row['success'] = code == 0 and row.get('prepared_report', {}).get('success') is True
                except Exception as error:
                    row['error'] = str(error)
                report['runs'].append(row)
                args.output.write_text(json.dumps(report, indent=2) + '\n')
                print(json.dumps({'pair': pair, 'variant': variant, 'success': row['success']}), flush=True)
    finally:
        sys.argv = original_argv
    report['artifacts_unchanged'] = all(warm.engines.digest(path) == hashes[name] for name, path in paths.items())
    report['success'] = report['artifacts_unchanged'] and len(report['runs']) == args.pairs * 2 and all(r['success'] for r in report['runs'])
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['success'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
