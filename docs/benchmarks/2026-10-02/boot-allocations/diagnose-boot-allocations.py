#!/usr/bin/env python3
"""Run the boot allocation probe through the owned prepared-guest harness."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('hypermachine', 'firecracker', 'kernel', 'initrd', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError('preserve previous evidence')
    spec = importlib.util.spec_from_file_location('prepared_boot_probe', Path(__file__).with_name('bench-prepared-engines.py'))
    warm = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(warm)
    binary = args.hypermachine.resolve(strict=True)
    original_popen, original_argv = warm.subprocess.Popen, sys.argv
    launches = []

    def popen(argv, **kwargs):
        if Path(argv[0]).resolve() == binary:
            kwargs['env'] = {**kwargs['env'], 'HM_BOOT_ALLOC_DIAGNOSTIC': '1'}
            process = original_popen(argv, **kwargs)
            launches.append(process)
            return process
        return original_popen(argv, **kwargs)

    prepared = args.output.with_name(args.output.stem + '-prepared.json')
    try:
        warm.subprocess.Popen = popen
        sys.argv = ['bench-prepared-engines.py', '--readiness-diagnostics', '--mapping-diagnostics',
                    '--pairs', '1', '--concurrency', '1', '--output', str(prepared)]
        for name in ('hypermachine', 'firecracker', 'kernel', 'initrd'):
            sys.argv += ['--' + name, str(getattr(args, name))]
        code = warm.main()
    finally:
        warm.subprocess.Popen, sys.argv = original_popen, original_argv
        for process in launches:
            if process.poll() is None:
                warm.engines.stop(process)
    cohort = json.loads(prepared.read_text())
    events = []
    for line in cohort.get('readiness_diagnostics', {}).get('node_log', '').splitlines():
        if line.startswith('HM_BOOT_ALLOC '):
            row = dict(field.split('=', 1) for field in line.split()[1:])
            events.append({key: value if key == 'stage' else int(value) for key, value in row.items()})
    required = {'image-read-before', 'image-read-live', 'highest-before', 'highest-live',
                'highest-dropped', 'initrd-copy-before', 'initrd-copy-live',
                'kernel-copy-before', 'kernel-copy-live'}
    report = {'diagnostic_only': True, 'runtime_change_adopted': False,
              'performance_win_established': False, 'cohort_success': cohort['success'],
              'cohort_artifact_sha256': cohort['artifact_sha256'],
              'driver_sha256': warm.engines.digest(Path(__file__).resolve()),
              'events': events, 'owned_node_exit_code': cohort.get('owned_node_exit_code'),
              'limitations': ['normal-thread process-wide GNU allocator snapshots',
                              'logging and concurrent work confound deltas',
                              'no performance ranking or allocation-to-PSS attribution']}
    report['success'] = code == 0 and cohort['success'] and len(launches) == 1 and required <= {e['stage'] for e in events}
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['success'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
