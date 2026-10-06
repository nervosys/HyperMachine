#!/usr/bin/env python3
"""Validate allocator isolation and summarize latency and idle memory tradeoffs."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import statistics

spec = importlib.util.spec_from_file_location('budget', Path(__file__).with_name('analyze-cold-start-limit.py'))
budget = importlib.util.module_from_spec(spec); spec.loader.exec_module(budget)


def require(value, message):
    if not value: raise ValueError(message)


def analyze(report):
    require(report['success'] == (report['artifacts_unchanged'] and all(row['success'] for row in report['runs'])), 'cohort success differs from runs')
    require(report['same_binary'] and report['baseline_limit'] == report['candidate_limit']
            and report['preflight_exit_code'] == 0 and all(report['allocation_symbols_bound'].values()), 'allocator/budget isolation missing')
    result = budget.analyze(report)
    pairs = []
    for pair in range(report['pairs']):
        runs = report['runs'][pair*2:pair*2+2]
        environments = {}
        for row in runs:
            candidate = row['variant'] == 'candidate'
            require(row['allocator_mapped'] is candidate, 'running allocator mismatch')
            environment = dict(row['daemon_environment'])
            require('MALLOC_CONF' not in environment and 'MALLOC_ARENA_MAX' not in environment, 'unexpected allocator tuning')
            require(environment.pop('LD_PRELOAD', None) == (report['allocator_path'] if candidate else None), 'allocator injection mismatch')
            environments[row['variant']] = environment
            if row['success']:
                require(not row['cleanup_errors'] and row['remaining_sandbox_count'] == 0 and row['daemon_exit_code'] == 0, 'successful run has failed cleanup')
            if row['success']:
                require(all(s['cleanup_success'] for s in row['batch']['samples']), 'successful run has guest cleanup failure')
                batch = row['batch']
                require(batch['memory_idle_requested_seconds'] == report['memory_idle_seconds'] == 5
                        and batch['memory_idle_actual_seconds'] >= 5, 'idle measurement missing')
                for name in ['idle_process_memory_kib', 'empty_process_memory_baseline_kib', 'incremental_idle_process_memory_kib']:
                    require(all(isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)
                                for v in batch[name].values()), 'invalid memory measurement')
                require(all(v >= 0 for name in ['idle_process_memory_kib', 'empty_process_memory_baseline_kib']
                            for v in batch[name].values()), 'negative absolute memory')
                require(all(batch['incremental_idle_process_memory_kib'][key] == value - batch['empty_process_memory_baseline_kib'][key]
                            for key, value in batch['idle_process_memory_kib'].items()), 'memory baseline subtraction differs')
        require(environments['baseline'] == environments['candidate'], 'daemon environments differ beyond allocator')
        item = {'pair': pair, 'complete_successful_pair': all(row['success'] for row in runs), 'candidate_held_pss_reduction_mib': None}
        if item['complete_successful_pair']:
            values = {row['variant']: row['batch']['idle_process_memory_kib']['Pss_kib'] / 1024 for row in runs}
            item['candidate_held_pss_reduction_mib'] = values['baseline'] - values['candidate']
        pairs.append(item)
    result['memory_pairs'] = pairs
    for variant in ['baseline', 'candidate']:
        rows = [r for r in report['runs'] if r['variant'] == variant and r['success']]
        result['variants'][variant]['median_held_pss_mib'] = statistics.median(r['batch']['idle_process_memory_kib']['Pss_kib']/1024 for r in rows) if rows else None
        result['variants'][variant]['median_incremental_pss_mib'] = statistics.median(r['batch']['incremental_idle_process_memory_kib']['Pss_kib']/1024 for r in rows) if rows else None
    result['allocator_change_adopted'] = False
    result['cleanup_verified'] = result['cleanup_verified'] and all(row.get('daemon_exit_code') == 0 for row in report['runs'])
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(not args.output.exists(), 'output exists; retain earlier analyses')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text())), indent=2) + '\n')
