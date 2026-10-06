#!/usr/bin/env python3
"""Compare owned/borrowed buffers in one executable with fresh matched controls."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys


def require(value, message):
    if not value:
        raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('binary', 'firecracker', 'kernel', 'initrd', 'output', 'build-context'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--pairs', type=int, default=2)
    parser.add_argument('--inner-pairs', type=int, default=2)
    parser.add_argument('--concurrency', type=int, default=8)
    parser.add_argument('--creation-diagnostics', action='store_true', help='Collect existing server stages; diagnostic-only, excluded from rankings')
    args = parser.parse_args()
    require(1 <= args.pairs <= 4 and 1 <= args.inner_pairs <= 16 and 1 <= args.concurrency <= 100, 'invalid bounded profile')
    require(not args.output.exists(), 'preserve previous evidence')
    spec = importlib.util.spec_from_file_location('buffer_modes_prepared', Path(__file__).with_name('bench-prepared-engines.py'))
    warm = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(warm)
    paths = {name: getattr(args, name).resolve(strict=True) for name in ('binary', 'firecracker', 'kernel', 'initrd')}
    for name, path in paths.items():
        setattr(args, name, path)
    paths.update(driver=Path(__file__).resolve(), coordinator=Path(warm.__file__).resolve(),
                 engines=Path(warm.engines.__file__).resolve(), firecracker_harness=Path(warm.fc.__file__).resolve(),
                 build_context=args.build_context.resolve(strict=True))
    hashes = {name: warm.engines.digest(path) for name, path in paths.items()}
    context = json.loads(args.build_context.read_text())
    require(context['binary_sha256'] == hashes['binary'] and context['build_exit_code'] == 0
            and context['intervention'] == 'same_binary_boot_buffer_modes'
            and context['production_runtime_changed'] is False, 'mode build binding differs')
    if context.get('diagnostic_only', False):
        require(args.creation_diagnostics, 'instrumented binary must remain diagnostic-only')
    affinity = sorted(os.sched_getaffinity(0))[:8]
    require(len(affinity) == 8, 'requires eight available CPUs')
    os.sched_setaffinity(0, affinity)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {'experiment_only': True, 'intervention': context['intervention'], 'runtime_change_adopted': False,
              'diagnostic_only': args.creation_diagnostics,
              'managed_competitor_win_established': False, 'artifact_sha256': hashes,
              'mode_source_sha256': context['mode_source_sha256'], 'mode_environment': 'HM_BOOT_IMAGE_MODE',
              'pairs': args.pairs, 'inner_pairs': args.inner_pairs, 'concurrency': args.concurrency,
              'cpu_affinity': affinity, 'runs': [], 'success': False,
              'order': 'fresh owned/borrowed AB/BA; internal HM/FC AB/BA',
              'limitations': ['Shared WSL nested KVM with uncontrolled background load',
                              'Owned mode is a counterfactual within the refactor, not the original accepted executable',
                              'Preparation and one-time activation logging excluded from restore timing',
                              'Repeated inner batches are not independent hosts; P99 is near the maximum for small samples']}
    original_argv, original_popen, original_batch = sys.argv, warm.subprocess.Popen, warm.batch
    try:
        for pair in range(args.pairs):
            for mode in (('owned', 'borrowed') if pair % 2 == 0 else ('borrowed', 'owned')):
                row = {'pair': pair, 'mode': mode, 'success': False}
                owned_nodes, activation = [], []

                def popen(argv, **kwargs):
                    if Path(argv[0]).resolve() == args.binary:
                        kwargs['env'] = {**kwargs['env'], 'HM_BOOT_IMAGE_MODE': mode}
                        process = original_popen(argv, **kwargs)
                        owned_nodes.append((process, Path(kwargs['stdout'].name)))
                        return process
                    return original_popen(argv, **kwargs)

                def batch(*positional, **keywords):
                    if not activation:
                        require(len(owned_nodes) == 1 and owned_nodes[0][0].poll() is None, 'owned node unavailable')
                        with owned_nodes[0][1].open('rb') as stream:
                            raw = stream.read(65537)
                        require(len(raw) <= 65536, 'startup mode capture exceeds bound')
                        lines = [line for line in raw.decode(errors='replace').splitlines() if line.startswith('HM_BOOT_IMAGE_MODE=')]
                        require(lines == ['HM_BOOT_IMAGE_MODE=' + mode], 'runtime activation differs')
                        activation.append({'requested': mode, 'observed': mode, 'captured_before_scoring': True})
                    return original_batch(*positional, **keywords)

                path = args.output.parent / f'{pair}-{mode}.json'
                try:
                    warm.subprocess.Popen, warm.batch = popen, batch
                    sys.argv = ['bench-prepared-engines.py', '--pairs', str(args.inner_pairs),
                                '--concurrency', str(args.concurrency), '--hypermachine', str(args.binary), '--output', str(path)]
                    for name in ('firecracker', 'kernel', 'initrd'):
                        sys.argv += ['--' + name, str(getattr(args, name))]
                    if args.creation_diagnostics:
                        sys.argv.append('--creation-diagnostics')
                    code = warm.main()
                    if path.exists():
                        row['prepared_report'] = json.loads(path.read_text())
                    row['activation'] = activation
                    row['success'] = code == 0 and row.get('prepared_report', {}).get('success') is True and len(activation) == 1
                except Exception as error:
                    row['error'] = str(error)
                finally:
                    warm.subprocess.Popen, warm.batch = original_popen, original_batch
                    for process, _ in owned_nodes:
                        if process.poll() is None:
                            warm.engines.stop(process)
                report['runs'].append(row)
                args.output.write_text(json.dumps(report, indent=2) + '\n')
                print(json.dumps({'pair': pair, 'mode': mode, 'success': row['success']}), flush=True)
    finally:
        sys.argv, warm.subprocess.Popen, warm.batch = original_argv, original_popen, original_batch
    report['artifacts_unchanged'] = all(warm.engines.digest(path) == hashes[name] for name, path in paths.items())
    report['success'] = report['artifacts_unchanged'] and len(report['runs']) == args.pairs * 2 and all(row['success'] for row in report['runs'])
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['success'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
