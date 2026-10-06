#!/usr/bin/env python3
"""Validate same-executable buffer modes, activation, guest state, and controls."""
import argparse
import importlib.util
import json
from pathlib import Path
import re
import statistics


def require(value, message):
    if not value:
        raise ValueError(message)


def analyze(report, directory, diagnostic=False):
    spec = importlib.util.spec_from_file_location('buffer_modes_warm_analysis', directory / 'analyze-prepared-engines.py')
    warm = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(warm)
    require(report['experiment_only'] is True and report['intervention'] == 'same_binary_boot_buffer_modes'
            and report['runtime_change_adopted'] is False and report['managed_competitor_win_established'] is False
            and report['artifacts_unchanged'] is True, 'input/claim differs')
    require(report.get('diagnostic_only', False) is diagnostic, 'diagnostic/scored classification differs')
    require(report['mode_environment'] == 'HM_BOOT_IMAGE_MODE', 'mode configuration differs')
    expected = {'crates/hv2-core/src/boot/linux.rs', 'crates/hv2-core/src/boot/source.rs', 'crates/hv2-core/src/backends/kvm.rs'}
    require(set(report['mode_source_sha256']) == expected
            and all(re.fullmatch('[0-9a-f]{64}', value) for value in report['mode_source_sha256'].values()), 'mode source binding differs')
    require(len(report['runs']) == report['pairs'] * 2 and report['success'] == all(row['success'] for row in report['runs']), 'outer runs inconsistent')
    values = {mode: [] for mode in ('owned', 'borrowed')}
    controls = {mode: [] for mode in values}
    held = {mode: [] for mode in values}
    empty = {mode: [] for mode in values}
    incremental = {mode: [] for mode in values}
    control_held = {mode: [] for mode in values}
    cohorts = []
    for index, row in enumerate(report['runs']):
        order = ('owned', 'borrowed') if index // 2 % 2 == 0 else ('borrowed', 'owned')
        mode = row['mode']
        require(row['pair'] == index // 2 and mode == order[index % 2], 'counterbalance differs')
        require(row['activation'] == [{'requested': mode, 'observed': mode, 'captured_before_scoring': True}], 'runtime activation unverified')
        raw = row['prepared_report']
        require(raw.get('diagnostic_only', False) is diagnostic and raw['pairs'] == report['inner_pairs']
                and raw['concurrency'] == report['concurrency'], 'nested scoring profile differs')
        require(raw['artifact_sha256']['hypermachine'] == report['artifact_sha256']['binary'], 'same executable invariant differs')
        require(all(raw['artifact_sha256'][key] == report['artifact_sha256'][key]
                    for key in ('firecracker', 'kernel', 'initrd', 'coordinator', 'engines', 'firecracker_harness')), 'nested inputs differ')
        checked = warm.analyze(raw)
        require(checked['cohort_success'] == row['success'] and checked['cleanup_verified']
                and checked.get('matched_guest_restore_contract') is True, 'guest contract or cleanup differs')
        for batch in raw['runs']:
            samples = [sample['ready_ms'] for sample in batch['samples'] if sample['success'] and sample['cleanup_success']]
            (values if batch['engine'] == 'hypermachine' else controls)[mode].extend(samples)
            if batch['engine'] == 'hypermachine' and batch['success']:
                held[mode].append(batch['held_process_memory_kib']['Pss_kib'] / 1024)
                empty[mode].append(batch['empty_process_memory_baseline_kib']['Pss_kib'] / 1024)
                incremental[mode].append(batch['incremental_process_memory_kib']['Pss_kib'] / 1024)
            if batch['engine'] == 'firecracker' and batch['success']:
                control_held[mode].append(batch['held_process_memory_kib']['Pss_kib'] / 1024)
        cohorts.append({'pair': row['pair'], 'mode': mode, 'activation_verified': True,
                        'cleanup_verified': checked['cleanup_verified'], 'engines': checked['engines']})
    planned = report['pairs'] * report['inner_pairs'] * report['concurrency']
    modes = {}
    for mode, samples in values.items():
        modes[mode] = {'planned': planned, 'passed': len(samples), 'failed': planned - len(samples),
                       'successful_mean_ms': statistics.mean(samples) if samples else None,
                       'successful_p50_ms': warm.percentile(samples, .5),
                       'successful_p95_ms': warm.percentile(samples, .95),
                       'successful_p99_ms': warm.percentile(samples, .99),
                       'median_held_pss_mib': statistics.median(held[mode]) if held[mode] else None,
                       'median_empty_pss_mib': statistics.median(empty[mode]) if empty[mode] else None,
                       'median_incremental_pss_mib': statistics.median(incremental[mode]) if incremental[mode] else None,
                       'firecracker_control_median_held_pss_mib': statistics.median(control_held[mode]) if control_held[mode] else None,
                       'firecracker_control_passed': len(controls[mode]),
                       'firecracker_control_mean_ms': statistics.mean(controls[mode]) if controls[mode] else None,
                       'firecracker_control_p99_ms': warm.percentile(controls[mode], .99)}
    pairs = []
    for pair in range(report['pairs']):
        by = {row['mode']: row['prepared_report'] for row in report['runs'] if row['pair'] == pair}
        item = {'pair': pair, 'complete': all(raw['success'] for raw in by.values())}
        if item['complete']:
            samples = {m: [s['ready_ms'] for b in raw['runs'] if b['engine'] == 'hypermachine' for s in b['samples']] for m, raw in by.items()}
            fc = {m: [s['ready_ms'] for b in raw['runs'] if b['engine'] == 'firecracker' for s in b['samples']] for m, raw in by.items()}
            memory = {m: statistics.median(b['held_process_memory_kib']['Pss_kib'] / 1024 for b in raw['runs'] if b['engine'] == 'hypermachine') for m, raw in by.items()}
            item.update(mean_reduction_ms=statistics.mean(samples['owned']) - statistics.mean(samples['borrowed']),
                        p99_reduction_ms=warm.percentile(samples['owned'], .99) - warm.percentile(samples['borrowed'], .99),
                        held_reduction_mib=memory['owned'] - memory['borrowed'],
                        firecracker_control_mean_shift_ms=statistics.mean(fc['borrowed']) - statistics.mean(fc['owned']))
        pairs.append(item)
    return {'modes': modes, 'pairs': pairs, 'cohorts': cohorts, 'cohort_success': report['success'],
            'diagnostic_only': diagnostic,
            'same_executable_verified': True, 'runtime_activation_verified': True,
            'cleanup_verified': all(c['cleanup_verified'] for c in cohorts),
            'runtime_change_adopted': False, 'managed_competitor_win_established': False,
            'latencies_conditional_on_success': True, 'memory_conditional_on_complete_batches': True}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(not args.output.exists(), 'preserve previous analysis')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text()), Path(__file__).parent), indent=2) + '\n')
