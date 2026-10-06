#!/usr/bin/env python3
"""Validate source-bound single-guest restore memory boundaries."""
import argparse
import importlib.util
import json
from pathlib import Path
import re

ANSI = re.compile(r'\x1b\[[0-9;]*m')
TRACE = re.compile(r'vm=(sbx-[0-9a-f]{20}) phase="(before_run|after_notice|after_exec)" size_kib=(\d+) rss_kib=(\d+) pss_kib=(\d+) private_dirty_kib=(\d+) shared_clean_kib=(\d+) shared_dirty_kib=(\d+) read_us=(\d+)')
PHASES = ['before_run', 'after_notice', 'after_exec']
COUNTERS = ['size_kib', 'rss_kib', 'pss_kib', 'private_dirty_kib', 'shared_clean_kib', 'shared_dirty_kib', 'read_us']

def require(value, message):
    if not value:
        raise ValueError(message)

def analyze(report, directory, context):
    require(context['diagnostic_only'] is True and context['production_runtime_changed'] is False,
            'diagnostic build classification differs')
    require(report['artifact_sha256']['hypermachine'] == context['binary_sha256'], 'diagnostic binary differs')
    require(report.get('diagnostic_only') is True and report['concurrency'] == 1,
            'requires single-guest diagnostic cohort')
    require(report.get('boundary_probe_activation') == {'environment': 'HM_RESTORE_MEMORY_DIAGNOSTICS=1',
            'owned_daemon_count': 1, 'injected_before_launch': True}, 'probe activation unverified')
    spec = importlib.util.spec_from_file_location('boundary_base_analysis', directory / 'analyze-prepared-engines.py')
    base = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(base)
    checked = base.analyze(report)
    require(checked['cohort_success'] and checked['cleanup_verified'], 'guest contract or cleanup failed')
    log = report['readiness_diagnostics']
    require(log['log_truncated'] is False and log['log_bytes'] == len(log['node_log'].encode('utf-8')),
            'diagnostic log incomplete')
    require(log['logging_may_affect_timing'] is True, 'diagnostic timing qualification missing')
    ids = [s['sandbox_id'] for batch in report['runs'] if batch['engine'] == 'hypermachine' for s in batch['samples']]
    require(len(ids) == len(set(ids)), 'duplicate scored VM')
    captured = {vm: [] for vm in ids}
    auxiliary = 0
    for raw in log['node_log'].splitlines():
        line = ANSI.sub('', raw)
        message = 'restore memory boundary diagnostic '
        if message not in line:
            continue
        require('hv2_core::vm:' in line, 'boundary target differs')
        payload = line.split(message, 1)[1]
        match = TRACE.fullmatch(payload)
        if match is None:
            # Source-preparation VMs have names other than scored sandbox IDs.
            require(not any('vm=' + vm + ' ' in payload for vm in ids), 'malformed scored boundary')
            auxiliary += 1
            continue
        vm, phase, *values = match.groups()
        if vm not in captured:
            auxiliary += 1
            continue
        counters = dict(zip(COUNTERS, map(int, values)))
        require(counters['size_kib'] == 1048576 and counters['pss_kib'] <= counters['rss_kib'] <= counters['size_kib'],
                'invalid mapping residency')
        require(counters['private_dirty_kib'] + counters['shared_clean_kib'] + counters['shared_dirty_kib'] <= counters['rss_kib'],
                'mapping residency components exceed RSS')
        captured[vm].append({'phase': phase, **counters})
    rows = []
    for vm, events in captured.items():
        require([event['phase'] for event in events] == PHASES, 'missing, duplicated or reordered boundary')
        rows.append({'vm': vm, 'boundaries': events,
                     'private_dirty_after_notice_minus_before_run_kib': events[1]['private_dirty_kib'] - events[0]['private_dirty_kib'],
                     'private_dirty_after_exec_minus_after_notice_kib': events[2]['private_dirty_kib'] - events[1]['private_dirty_kib']})
    return {'diagnostic_only': True, 'performance_win_established': False, 'runtime_change_adopted': False,
            'cleanup_verified': True, 'attempts': sum(e['attempted'] for e in checked['engines'].values()),
            'hypermachine_boundary_guests': len(rows), 'auxiliary_events_excluded': auxiliary, 'guests': rows,
            'limitations': ['Guest/device activity continues during the after-notice and after-exec observations',
                           'Counters describe residency, not every write or its code origin',
                           'Private dirty can include uniquely mapped dirty file-cache pages; anonymous/COW ownership is not measured',
                           'Before-run mapping may contain preexisting private state from required restoration',
                           'Procfs reads and debug logs perturb timing; excluded from rankings',
                           'No equivalent Firecracker boundary observations in this study']}

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--build-context', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(not args.output.exists(), 'preserve previous analysis')
    result = analyze(json.loads(args.report.read_text()), Path(__file__).parent,
                     json.loads(args.build_context.read_text()))
    args.output.write_text(json.dumps(result, indent=2) + '\n')
