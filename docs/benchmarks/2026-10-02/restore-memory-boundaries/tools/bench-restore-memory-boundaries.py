#!/usr/bin/env python3
"""Activate probes only in the owned diagnostic daemon's hermetic environment."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

def main():
    parser = argparse.ArgumentParser(description=__doc__, add_help=False)
    parser.add_argument('--build-context', type=Path, required=True)
    args, forwarded = parser.parse_known_args()
    def option(name): return forwarded[forwarded.index(name) + 1]
    if option('--concurrency') != '1' or '--readiness-diagnostics' not in forwarded:
        raise ValueError('requires single-guest readiness-diagnostic profile')
    binary = Path(option('--hypermachine')).resolve(strict=True)
    output = Path(option('--output'))
    context = json.loads(args.build_context.read_text())
    if context.get('diagnostic_only') is not True:
        raise ValueError('requires diagnostic build context')
    spec = importlib.util.spec_from_file_location('boundary_coordinator', Path(__file__).with_name('bench-prepared-engines.py'))
    warm = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(warm)
    if warm.engines.digest(binary) != context['binary_sha256']:
        raise ValueError('diagnostic executable differs')
    original, argv = warm.subprocess.Popen, sys.argv
    activated = []
    def popen(command, **kwargs):
        if Path(command[0]).resolve() == binary:
            kwargs['env'] = {**kwargs['env'], 'HM_RESTORE_MEMORY_DIAGNOSTICS': '1'}
            process = original(command, **kwargs)
            activated.append(process.pid)
            return process
        return original(command, **kwargs)
    try:
        warm.subprocess.Popen = popen
        sys.argv = ['bench-prepared-engines.py', *forwarded]
        code = warm.main()
    finally:
        warm.subprocess.Popen, sys.argv = original, argv
    if len(activated) != 1:
        raise ValueError('owned diagnostic daemon activation differs')
    report = json.loads(output.read_text())
    report['boundary_probe_activation'] = {'environment': 'HM_RESTORE_MEMORY_DIAGNOSTICS=1',
                                           'owned_daemon_count': 1, 'injected_before_launch': True}
    output.write_text(json.dumps(report, indent=2) + '\n')
    return code

if __name__ == '__main__':
    raise SystemExit(main())
