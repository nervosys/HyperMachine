#!/usr/bin/env python3
"""Validate cold-boot environment isolation, complete accounting and held PSS."""
import argparse
import importlib.util
import json
from pathlib import Path
import statistics


def require(value, message):
    if not value:
        raise ValueError(message)


def analyze(report, directory):
    spec = importlib.util.spec_from_file_location('cold_analysis', directory / 'analyze-budget-engines.py')
    cold = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(cold)
    require(report['same_binary'] is True and report['artifacts_unchanged'] is True
            and report['runtime_change_adopted'] is False and report['managed_competitor_win_established'] is False,
            'artifact or adoption claim differs')
    require(report['candidate_environment'] == {'MALLOC_MMAP_THRESHOLD_': '131072'}
            and report['libc'][0] == 'glibc' and report['intervention'] == 'static_glibc_mmap_threshold_131072',
            'allocator policy differs')
    require(type(report['pairs']) is int and 1 <= report['pairs'] <= 8
            and type(report['concurrency']) is int and 1 <= report['concurrency'] <= 100,
            'profile differs')
    require(len(report['cpu_affinity']) == len(set(report['cpu_affinity'])) == 8, 'affinity differs')
    require(len(report['runs']) == report['pairs'] * 2, 'missing outer run')
    result = {'cohort_success': report['success'], 'cleanup_verified': True, 'variants': {}, 'pairs': [],
              'runtime_change_adopted': False, 'managed_competitor_win_established': False,
              'latencies_conditional_on_success': True, 'memory_conditional_on_complete_batches': True}
    metrics = {v: {'hypermachine': [], 'firecracker': [], 'held': [], 'empty': []}
               for v in ('baseline', 'candidate')}
    paired = {}
    for index, row in enumerate(report['runs']):
        pair = index // 2
        order = ('baseline', 'candidate') if pair % 2 == 0 else ('candidate', 'baseline')
        variant = order[index % 2]
        require(row['pair'] == pair and row['variant'] == variant, 'counterbalance differs')
        require(row['owned_daemon_exit_codes'] == [0] and len(row['activations']) == 1, 'owned process differs')
        activation = row['activations'][0]
        require(type(activation['pid']) is int and activation['pid'] > 1
                and activation['executable_sha256'] == report['artifact_sha256']['daemon'], 'owned executable differs')
        environment = dict(activation['environment'])
        require(environment.pop('MALLOC_MMAP_THRESHOLD_', None) == ('131072' if variant == 'candidate' else None),
                'running threshold differs')
        require(set(environment) == {'PATH', 'HV2_KERNEL', 'HV2_INITRD', 'RUST_LOG'}
                and environment['RUST_LOG'] == 'warn', 'unexpected environment')
        raw = row['cold_report']
        require(raw['lifecycle'] == 'native-cold-create-to-command-bursts' and raw['pairs'] == 2
                and raw['concurrency'] == report['concurrency'] and raw['cold_start_concurrency'] == 16
                and raw['daemon_allocator_arena_max'] is None and raw['daemon_log_filter'] == 'warn', 'cold profile differs')
        require(raw['driver_cpu_affinity'] == report['cpu_affinity'] and raw['artifacts_unchanged'] is True
                and raw['daemon_exit_code'] == 0, 'host identity or exit differs')
        base = raw['hypermachine_template_preflight']
        require(base['snapshot'] is False and base['cpuCount'] == 1 and base['memoryMB'] == 1024
                and 'base' in base['aliases'], 'cold template resources differ')
        argv = raw['daemon_argv']
        for flag, value in (('--memory-mb', '1024'), ('--cpu-cores', '1'), ('--capacity', '128')):
            require(argv.count(flag) == 1 and argv[argv.index(flag) + 1] == value, 'actual daemon resources differ')
        require(argv.count('--no-template') == 1, 'cold template selection differs')
        for batch in raw['batches']:
            require(batch['success'] == (not batch['error'] and all(s['success'] and s['cleanup_success']
                    for s in batch['samples'])), 'batch success accounting differs')
        require(raw['success'] == (not raw['setup_error'] and not raw['cleanup_errors']
                and raw['artifacts_unchanged'] and len(raw['batches']) == 4
                and all(b['success'] for b in raw['batches'])), 'cold success accounting differs')
        for nested, outer in {'hypermachine': 'daemon', 'firecracker': 'firecracker', 'kernel': 'kernel',
                              'initrd': 'initrd', 'harness': 'coordinator', 'shared_harness': 'engines',
                              'firecracker_harness': 'firecracker_harness'}.items():
            require(raw['artifact_sha256'][nested] == report['artifact_sha256'][outer], 'nested input differs')
        checked = cold.analyze(raw)
        require(checked['cleanup_verified'] and checked['cohort_success'] == row['success']
                and not raw['setup_error'] and not raw['cleanup_errors'], 'cleanup or success differs')
        item = {'environment': environment, 'hypermachine': [], 'firecracker': [], 'held': [], 'empty': []}
        for batch in raw['batches']:
            engine = batch['engine']
            values = [s['ready_ms'] for s in batch['samples'] if s['success'] and s['cleanup_success']]
            item[engine].extend(values)
            metrics[variant][engine].extend(values)
            if engine == 'hypermachine' and batch['success']:
                for key, field in (('held', 'idle_process_memory_kib'), ('empty', 'empty_process_memory_baseline_kib')):
                    value = batch[field]['Pss_kib'] / 1024
                    item[key].append(value)
                    metrics[variant][key].append(value)
        paired.setdefault(pair, {})[variant] = item
    require(report['success'] == all(row['success'] for row in report['runs']), 'outer success differs')
    planned = report['pairs'] * 2 * report['concurrency']
    for variant, values in metrics.items():
        result['variants'][variant] = {'engines': {}, 'median_held_pss_mib': statistics.median(values['held']) if values['held'] else None,
                                       'median_empty_pss_mib': statistics.median(values['empty']) if values['empty'] else None}
        for engine in ('hypermachine', 'firecracker'):
            samples = values[engine]
            result['variants'][variant]['engines'][engine] = {'planned': planned, 'passed': len(samples), 'failed': planned-len(samples),
                'successful_mean_ms': statistics.mean(samples) if samples else None,
                'successful_p50_ms': cold.percentile(samples, .5), 'successful_p99_ms': cold.percentile(samples, .99)}
    for pair, values in paired.items():
        b, c = values['baseline'], values['candidate']
        require(b['environment'] == c['environment'], 'paired environment differs')
        complete = all(len(values[v][e]) == 2 * report['concurrency'] for v in ('baseline', 'candidate')
                       for e in ('hypermachine', 'firecracker'))
        row = {'pair': pair, 'complete': complete}
        if complete:
            row.update(mean_reduction_ms=statistics.mean(b['hypermachine'])-statistics.mean(c['hypermachine']),
                       p99_reduction_ms=cold.percentile(b['hypermachine'], .99)-cold.percentile(c['hypermachine'], .99),
                       held_reduction_mib=statistics.median(b['held'])-statistics.median(c['held']),
                       empty_reduction_mib=statistics.median(b['empty'])-statistics.median(c['empty']),
                       firecracker_control_mean_shift_ms=statistics.mean(c['firecracker'])-statistics.mean(b['firecracker']))
        result['pairs'].append(row)
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(not args.output.exists(), 'preserve previous analysis')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_bytes()), Path(__file__).parent), indent=2) + '\n')
