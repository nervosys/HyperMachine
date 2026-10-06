#!/usr/bin/env python3
"""Validate cold admission cohorts including matched held/empty daemon PSS."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import statistics

spec = importlib.util.spec_from_file_location('budget', Path(__file__).with_name('analyze-cold-start-limit.py'))
budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(budget)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def analyze(report):
    require(report['same_binary'], 'requires identical binaries')
    require(report['success'] == (report['artifacts_unchanged'] and all(row['success'] for row in report['runs'])), 'cohort success inconsistent')
    require(number(report['memory_idle_seconds']) and 1 <= report['memory_idle_seconds'] <= 30, 'held-memory interval unavailable')
    result = budget.analyze(report)
    for row in report['runs']:
        if not row['success']:
            continue
        batch = row['batch']
        require(row['daemon_exit_code'] == 0 and row['remaining_sandbox_count'] == 0 and not row['cleanup_errors'], 'successful run has failed cleanup')
        require(batch['memory_idle_requested_seconds'] == report['memory_idle_seconds'] and number(batch['memory_idle_actual_seconds']) and batch['memory_idle_actual_seconds'] >= report['memory_idle_seconds'], 'hold was shortened')
        require(number(batch['idle_memory_read_duration_ms']) and batch['idle_memory_read_duration_ms'] >= 0, 'invalid memory read duration')
        memory = batch['idle_process_memory_kib']; baseline = batch['empty_process_memory_baseline_kib']; incremental = batch['incremental_idle_process_memory_kib']
        require(memory.keys() == baseline.keys() == incremental.keys() and 'Pss_kib' in memory, 'memory fields differ')
        require(all(number(v) and v >= 0 for source in [memory, baseline] for v in source.values()), 'invalid absolute memory')
        require(all(number(incremental[k]) and incremental[k] == v - baseline[k] for k,v in memory.items()), 'baseline subtraction differs')
        require(baseline == row['empty_node_memory_before_batch_kib'], 'wrong empty-node baseline')
    result['memory_idle_seconds'] = report['memory_idle_seconds']
    result['memory_pairs'] = []
    for pair in result['pairs']:
        runs = report['runs'][pair['pair']*2:pair['pair']*2+2]
        item = {'pair':pair['pair'], 'complete_successful_pair':pair['complete_successful_pair'], 'candidate_held_pss_reduction_mib':None, 'candidate_incremental_pss_reduction_mib':None}
        if pair['complete_successful_pair']:
            for kind, field in [('held','idle_process_memory_kib'), ('incremental','incremental_idle_process_memory_kib')]:
                values = {row['variant']:row['batch'][field]['Pss_kib']/1024 for row in runs}
                item['candidate_'+kind+'_pss_reduction_mib'] = values['baseline'] - values['candidate']
        result['memory_pairs'].append(item)
    for variant in ['baseline','candidate']:
        rows = [r for r in report['runs'] if r['variant'] == variant and r['success']]
        for kind, field in [('held','idle_process_memory_kib'), ('incremental','incremental_idle_process_memory_kib')]:
            result['variants'][variant]['median_'+kind+'_pss_mib'] = statistics.median(r['batch'][field]['Pss_kib']/1024 for r in rows) if rows else None
    result['cleanup_verified'] = result['cleanup_verified'] and all(row.get('daemon_exit_code') == 0 for row in report['runs'])
    result['memory_conditional_on_complete_successful_runs'] = True
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error('output exists; preserve earlier analysis')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text())),indent=2)+'\n')
