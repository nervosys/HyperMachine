#!/usr/bin/env python3
"""Compare libc and an explicitly supplied jemalloc on the same owned daemon."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess


def require(value, message):
    if not value: raise ValueError(message)


spec = importlib.util.spec_from_file_location('budget', Path(__file__).with_name('bench-cold-start-limit.py'))
budget = importlib.util.module_from_spec(spec); spec.loader.exec_module(budget)
comparison, engines = budget.comparison, budget.engines


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['daemon', 'allocator', 'kernel', 'initrd', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--pairs', type=int, default=4)
    parser.add_argument('--concurrency', type=int, default=100)
    parser.add_argument('--cold-start-concurrency', type=int, default=16)
    args = parser.parse_args()
    require(1 <= args.pairs <= 10 and 1 <= args.concurrency <= 100 and 1 <= args.cold_start_concurrency <= 1024, 'invalid experiment limits')
    os.umask(0o077)
    args.output = args.output.resolve(); args.output.mkdir(parents=True, exist_ok=False)
    inputs = {name: getattr(args, name).resolve(strict=True) for name in ['daemon', 'allocator', 'kernel', 'initrd']}
    require(inputs['allocator'].is_file() and not any(c in str(inputs['allocator']) for c in [' ', ':', '\n']), 'allocator path must name one preload library')
    for name, path in inputs.items(): setattr(args, name, path)
    args.candidate_limit = args.baseline_limit = args.cold_start_concurrency
    args.memory_idle_seconds, args.daemon_log_filter = 5, 'warn'
    affinity = sorted(os.sched_getaffinity(0))[:8]; os.sched_setaffinity(0, affinity)
    inputs.update(harness=Path(__file__).resolve(), budget=Path(budget.__file__).resolve(),
        comparison=Path(comparison.__file__).resolve(), burst=Path(comparison.burst.__file__).resolve(),
        shared=Path(engines.__file__).resolve(), firecracker=Path(comparison.burst.fc.__file__).resolve())
    identities = {name: engines.digest(path) for name, path in inputs.items()}
    identities.update(baseline=identities['daemon'], candidate=identities['daemon'])
    report = dict(purpose='same-daemon allocator experiment; no competitor win claim', artifact_sha256=identities,
        driver_cpu_affinity=affinity, added_CPU_load=False, concurrency=args.concurrency, pairs=args.pairs,
        candidate_limit=args.candidate_limit, baseline_limit=args.baseline_limit, same_binary=True,
        guest_readiness_timeout_s=15, cpu_count=1, memory_mb=1024, queue_included_in_ready_ms=True,
        memory_idle_seconds=5, order='fresh-daemon AB/BA', allocator_path=str(args.allocator),
        allocator_configuration='jemalloc defaults; no MALLOC_CONF', runs=[], success=False,
        limitations=['Shared nested-KVM host and uncontrolled host background load',
            'Conditional latency summaries exclude failed attempts; all attempts retained',
            'PSS excludes kernel allocations; fixed idle hold is not fleet density',
            'No managed competitor endpoint or production allocator adoption'])
    # Loader diagnostics run only during this preflight, never during timed bursts.
    preflight = subprocess.run([str(args.daemon), '--help'], env={'PATH': '/usr/bin:/bin',
        'LD_PRELOAD': str(args.allocator), 'LD_DEBUG': 'bindings', 'LD_BIND_NOW': '1'}, capture_output=True, timeout=30)
    (args.output / 'bindings.log').write_bytes(preflight.stderr)
    require(preflight.returncode == 0, 'allocator loader preflight failed')
    bindings = preflight.stderr.decode(errors='replace')
    report['allocation_symbols_bound'] = {name: any(str(args.daemon) in line and str(args.allocator) in line
        and "normal symbol `" + name + "'" in line for line in bindings.splitlines()) for name in ['malloc', 'calloc', 'realloc', 'free']}
    require(all(report['allocation_symbols_bound'].values()), 'allocator interposition not proven')
    for pair in range(args.pairs):
        for variant in (['baseline', 'candidate'] if pair % 2 == 0 else ['candidate', 'baseline']):
            original_launch, original_batch = comparison.subprocess.Popen, comparison.burst.batch
            observed = {}
            def launch(command, *positional, **kwargs):
                if Path(command[0]).resolve() == args.daemon and '--no-template' in command:
                    environment = dict(kwargs['env'])
                    require(not any(name in environment for name in ['LD_PRELOAD', 'MALLOC_CONF', 'MALLOC_ARENA_MAX']), 'baseline allocator environment contaminated')
                    if variant == 'candidate': environment['LD_PRELOAD'] = str(args.allocator)
                    kwargs['env'] = environment
                    observed['daemon_environment'] = environment
                return original_launch(command, *positional, **kwargs)
            def batch(*positional, **kwargs):
                mappings = Path(f'/proc/{args.node_pid}/maps').read_text()
                mapped = any(line.split()[-1] == str(args.allocator) for line in mappings.splitlines() if line.split())
                observed['allocator_mapped'] = mapped
                require(mapped == (variant == 'candidate'), 'running allocator mapping mismatch')
                return original_batch(*positional, **kwargs)
            comparison.subprocess.Popen, comparison.burst.batch = launch, batch
            try: row = budget.run(args, args.daemon, variant, pair)
            finally: comparison.subprocess.Popen, comparison.burst.batch = original_launch, original_batch
            row.update(observed); report['runs'].append(row)
            (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
            print(json.dumps(dict(pair=pair, variant=variant, success=row['success'])), flush=True)
    report['artifacts_unchanged'] = all(engines.digest(path) == identities[name] for name, path in inputs.items())
    report['success'] = report['artifacts_unchanged'] and all(row['success'] for row in report['runs'])
    report['ready_ms'] = {variant: comparison.burst.fc.summary([sample['ready_ms'] for row in report['runs'] if row['variant'] == variant
        for sample in row.get('batch', {}).get('samples', []) if sample['success'] and sample['cleanup_success']]) for variant in ['baseline', 'candidate']}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['success'] else 1


if __name__ == '__main__': raise SystemExit(main())
