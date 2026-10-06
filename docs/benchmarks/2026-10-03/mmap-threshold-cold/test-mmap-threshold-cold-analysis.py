#!/usr/bin/env python3
"""Reject altered allocator benchmark claims, including under Python -O."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('report', type=Path)
args = parser.parse_args()
spec = importlib.util.spec_from_file_location('threshold_analysis', Path(__file__).with_name('analyze-mmap-threshold-cold.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
original = json.loads(args.report.read_bytes())
module.analyze(original, Path(__file__).parent)
mutations = {
    'adoption': lambda r: r.update(runtime_change_adopted=True),
    'managed-win': lambda r: r.update(managed_competitor_win_established=True),
    'changed-artifacts': lambda r: r.update(artifacts_unchanged=False),
    'threshold': lambda r: r.update(candidate_environment={'MALLOC_MMAP_THRESHOLD_': '1'}),
    'order': lambda r: r['runs'][0].update(variant='candidate'),
    'cleanup': lambda r: r['runs'][0].update(owned_daemon_exit_codes=[-15]),
    'executable': lambda r: r['runs'][0]['activations'][0].update(executable_sha256='0' * 64),
    'probe': lambda r: r['runs'][0]['activations'][0]['environment'].update(LD_PRELOAD='/probe.so'),
    'diagnostic': lambda r: r['runs'][0]['cold_report'].update(daemon_log_filter='trace'),
    'queue-timing': lambda r: r['runs'][0]['cold_report'].update(queue_included_in_ready_ms=False),
    'nested-input': lambda r: r['runs'][0]['cold_report']['artifact_sha256'].update(kernel='0' * 64),
    'affinity': lambda r: r.update(cpu_affinity=[0] * 8),
    'missing-run': lambda r: r['runs'].pop(),
    'cold-budget': lambda r: r['runs'][0]['cold_report'].update(cold_start_concurrency=32),
    'idle-hold': lambda r: r['runs'][0]['cold_report'].update(memory_idle_seconds=0),
    'template-resources': lambda r: r['runs'][0]['cold_report']['hypermachine_template_preflight'].update(memoryMB=512),
    'cold-selection': lambda r: r['runs'][0]['cold_report']['hypermachine_template_preflight'].update(snapshot=True),
    'batch-accounting': lambda r: r['runs'][0]['cold_report']['batches'][0].update(success=False),
    'false-success': lambda r: r['runs'][0]['cold_report']['batches'][0]['samples'][0].update(cleanup_success=False),
}
for name, mutate in mutations.items():
    altered = copy.deepcopy(original)
    mutate(altered)
    try:
        module.analyze(altered, Path(__file__).parent)
    except (ValueError, KeyError, TypeError):
        continue
    raise ValueError('accepted altered evidence: ' + name)
print(json.dumps({'valid_report_passed': True, 'negative_contract_checks': len(mutations)}))
