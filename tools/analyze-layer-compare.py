#!/usr/bin/env python3
"""Validate and summarize counterbalanced layer-compare cohorts and controls."""
import argparse
import importlib.util
import json
from pathlib import Path
import re
import statistics


def require(value, message):
    if not value:
        raise ValueError(message)


def analyze(report, directory):
    spec = importlib.util.spec_from_file_location('borrowed_warm_analysis', directory / 'analyze-prepared-engines.py')
    warm = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(warm)
    require(report['artifacts_unchanged'] is True and report['runtime_change_adopted'] is False
            and report['managed_competitor_win_established'] is False, 'input/claim differs')
    require(report['intervention'] == 'layered_restore_compare_before_write', 'intervention differs')
    require(len(report['runs']) == report['pairs'] * 2, 'planned outer run missing')
    expected_sources = {'crates/hv2-core/src/vm.rs'}
    require(set(report['candidate_source_sha256']) == expected_sources
            and all(re.fullmatch('[0-9a-f]{64}', sha) for sha in report['candidate_source_sha256'].values()), 'source binding differs')
    require(report['success'] == all(row['success'] for row in report['runs']), 'outer success inconsistent')
    values = {v: [] for v in ('baseline', 'candidate')}
    controls = {v: [] for v in values}
    memory = {v: [] for v in values}
    empty = {v: [] for v in values}
    captures = {v: [] for v in values}
    named_bytes = {v: [] for v in values}
    cohorts = []
    for index, row in enumerate(report['runs']):
        order = ('baseline', 'candidate') if index // 2 % 2 == 0 else ('candidate', 'baseline')
        variant = row['variant']
        require(row['pair'] == index // 2 and variant == order[index % 2], 'counterbalance differs')
        raw = row['prepared_report']
        require(raw.get('diagnostic_only', False) is False and raw['pairs'] == 2
                and raw['concurrency'] == report['concurrency'], 'nested scoring profile differs')
        require(raw['artifact_sha256']['hypermachine'] == report['artifact_sha256'][variant], 'variant binary differs')
        require(all(raw['artifact_sha256'][key] == report['artifact_sha256'][key]
                    for key in ('firecracker', 'kernel', 'initrd', 'coordinator', 'engines', 'firecracker_harness')), 'nested inputs differ')
        checked = warm.analyze(raw)
        require(raw.get('resource_validation_phase')=='after_all_timed_attempts', 'corrected resource validation missing')
        prep = raw['preparation']['hypermachine']
        capture = prep['named_capture_ms']
        require(warm.number(capture) and capture >= 0, 'invalid named capture cost')
        files = {name: value for name,value in prep['source_files'].items() if Path(name).name.startswith('warm-benchmark-')}
        images = [value for name,value in files.items() if name.endswith('.snap.mem')]
        require(len(images)==0, 'named image layout differs')
        if images:
            require(images[0]['bytes']==1024*1024*1024, 'named image size differs')
        require(files and all(isinstance(f['bytes'],int) and not isinstance(f['bytes'],bool) and f['bytes']>=0 for f in files.values()), 'named source byte catalog differs')
        captures[variant].append(capture)
        named_bytes[variant].append(sum(f['bytes'] for f in files.values()))
        require(checked['cohort_success'] == row['success'] and checked['cleanup_verified']
                and checked.get('matched_guest_restore_contract') is True, 'nested contract or cleanup differs')
        for batch in raw['runs']:
            samples = [sample['ready_ms'] for sample in batch['samples'] if sample['success'] and sample['cleanup_success']]
            (values if batch['engine'] == 'hypermachine' else controls)[variant].extend(samples)
            if batch['engine'] == 'hypermachine' and batch['success']:
                memory[variant].append(batch['held_process_memory_kib']['Pss_kib'] / 1024)
                empty[variant].append(batch['empty_process_memory_baseline_kib']['Pss_kib'] / 1024)
        cohorts.append({'pair': row['pair'], 'variant': variant, 'cleanup_verified': checked['cleanup_verified'], 'engines': checked['engines']})
    planned = report['pairs'] * 2 * report['concurrency']
    variants = {}
    for variant, samples in values.items():
        variants[variant] = {'planned': planned, 'passed': len(samples), 'failed': planned - len(samples),
                             'successful_mean_ms': statistics.mean(samples) if samples else None,
                             'successful_p50_ms': warm.percentile(samples, .5),
                             'successful_p95_ms': warm.percentile(samples, .95),
                             'successful_p99_ms': warm.percentile(samples, .99),
                             'median_held_pss_mib': statistics.median(memory[variant]) if memory[variant] else None,
                             'median_empty_pss_mib': statistics.median(empty[variant]) if empty[variant] else None,
                             'median_named_capture_ms': statistics.median(captures[variant]),
                             'median_named_source_logical_bytes': statistics.median(named_bytes[variant]),
                             'firecracker_control_passed': len(controls[variant]),
                             'firecracker_control_mean_ms': statistics.mean(controls[variant]) if controls[variant] else None,
                             'firecracker_control_p99_ms': warm.percentile(controls[variant], .99)}
    pairs = []
    for pair in range(report['pairs']):
        by = {row['variant']: row['prepared_report'] for row in report['runs'] if row['pair'] == pair}
        item = {'pair': pair, 'complete': all(raw['success'] for raw in by.values())}
        if item['complete']:
            samples = {v: [s['ready_ms'] for b in raw['runs'] if b['engine'] == 'hypermachine' for s in b['samples']] for v, raw in by.items()}
            fc = {v: [s['ready_ms'] for b in raw['runs'] if b['engine'] == 'firecracker' for s in b['samples']] for v, raw in by.items()}
            held = {v: statistics.median(b['held_process_memory_kib']['Pss_kib'] / 1024 for b in raw['runs'] if b['engine'] == 'hypermachine') for v, raw in by.items()}
            item.update(mean_reduction_ms=statistics.mean(samples['baseline']) - statistics.mean(samples['candidate']),
                        p99_reduction_ms=warm.percentile(samples['baseline'], .99) - warm.percentile(samples['candidate'], .99),
                        held_reduction_mib=held['baseline'] - held['candidate'],
                        firecracker_control_mean_shift_ms=statistics.mean(fc['candidate']) - statistics.mean(fc['baseline']))
        pairs.append(item)
    return {'variants': variants, 'pairs': pairs, 'cohorts': cohorts, 'cohort_success': report['success'],
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
